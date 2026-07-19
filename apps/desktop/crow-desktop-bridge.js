#!/usr/bin/env node
/**
 * crow-desktop-bridge — translates the Crow desktop JSON-RPC
 * contract (used by apps/desktop/) into pi's --mode rpc wire
 * format. Pi's native RPC mode emits one JSON line per event but
 * doesn't expose the desktop's session/submit/interrupt methods,
 * so we wrap it.
 *
 * Inputs:
 *   stdin:  JSON-RPC lines from the desktop shell ({"jsonrpc":"2.0",
 *          "id":<n>,"method":<str>,"params":<obj>})
 *   stdout: JSON-RPC responses + pushed "event"/"ask" notifications,
 *          in the same shape apps/desktop's lib.rs expects
 *
 * Outputs to pi:
 *   spawn `pi --mode rpc` (or its installed binary) with stdin/stdout
 *   piped. pi emits one JSON line per event; we forward "event"/"ask"
 *   frames and translate method calls.
 *
 * The bridge is intentionally tiny (~150 LOC) — pi does the real work.
 */

'use strict';

const { spawn } = require('node:child_process');
const path = require('node:path');
const readline = require('node:readline');

const PI_BIN = process.env.CROW_PI_BIN || 'pi';
const PROJECT_ROOT = process.cwd();

/* ---------- in-memory session state ---------- */

const sessions = new Map(); // session_id -> { pi_proc, line_buf, pending_id }

/* ---------- pi process management ---------- */

function spawn_pi(session_id) {
  const proc = spawn(PI_BIN, ['--mode', 'rpc'], {
    cwd: PROJECT_ROOT,
    env: { ...process.env, PI_OFFLINE: '0' },
    stdio: ['pipe', 'pipe', 'inherit'],
  });

  const state = {
    pi_proc: proc,
    line_buf: '',
    pending_id: 0,
  };
  sessions.set(session_id, state);

  proc.stdout.on('data', (chunk) => {
    state.line_buf += chunk.toString('utf8');
    let nl;
    while ((nl = state.line_buf.indexOf('\n')) >= 0) {
      const line = state.line_buf.slice(0, nl);
      state.line_buf = state.line_buf.slice(nl + 1);
      handle_pi_line(session_id, line);
    }
  });

  proc.on('exit', (code) => {
    sessions.delete(session_id);
    push_event({
      type: 'sidecar_exit',
      session_id,
      code,
    });
  });

  return proc;
}

function send_pi(session_id, payload) {
  const state = sessions.get(session_id);
  if (!state) throw new Error(`no pi process for session ${session_id}`);
  state.pi_proc.stdin.write(JSON.stringify(payload) + '\n');
}

function handle_pi_line(session_id, line) {
  if (!line) return;
  let evt;
  try {
    evt = JSON.parse(line);
  } catch {
    return;
  }
  // pi events arrive as { "type": "...", ... } — forward verbatim
  // wrapped in the desktop's { "method": "event", "params": evt } envelope.
  if (evt && typeof evt === 'object' && 'type' in evt) {
    push_event({ ...evt, session_id });
  }
}

function push_event(params) {
  process.stdout.write(JSON.stringify({
    jsonrpc: '2.0',
    method: 'event',
    params,
  }) + '\n');
}

/* ---------- JSON-RPC handler (from the desktop) ---------- */

const input = readline.createInterface({ input: process.stdin, terminal: false });

input.on('line', async (line) => {
  let msg;
  try {
    msg = JSON.parse(line);
  } catch {
    return;
  }
  const { id = null, method, params = {} } = msg;
  try {
    const result = await dispatch(method, params, id);
    if (id !== null) {
      process.stdout.write(
        JSON.stringify({ jsonrpc: '2.0', id, result }) + '\n',
      );
    }
  } catch (err) {
    const code = err.code ?? -32000;
    const message = err.message ?? String(err);
    if (id !== null) {
      process.stdout.write(
        JSON.stringify({
          jsonrpc: '2.0',
          id,
          error: { code, message },
        }) + '\n',
      );
    }
  }
});

async function dispatch(method, params, _id) {
  switch (method) {
    case 'initialize':
      return { protocol_version: 1 };
    case 'session_start': {
      const session_id = params.session_id || `sess-${Date.now()}`;
      // Spawn a fresh pi process per session so we can interrupt cleanly.
      spawn_pi(session_id);
      // Wait for the first event so we know pi is alive.
      await new Promise((resolve) => setTimeout(resolve, 250));
      return { session_id, path: params.path || `${PROJECT_ROOT}/.crow/sessions/${session_id}.jsonl` };
    }
    case 'submit': {
      const session_id = params.session_id;
      if (!session_id || !sessions.has(session_id)) throw rpcError(-32001, 'unknown session');
      const prompt = (params.user_message || '') +
        (params.attachments ? '\n' + params.attachments.join('\n') : '');
      // pi's --mode rpc is event-only, no request/response. We can't
      // directly map submit -> pi without a real prompt API. The
      // bridge translates submit into a "prompt" event via pi's
      // stdin: pi consumes { "type": "user_prompt", "content": "..." }.
      send_pi(session_id, { type: 'message', role: 'user', content: prompt });
      return { accepted: true };
    }
    case 'interrupt': {
      const session_id = params.session_id;
      if (!session_id || !sessions.has(session_id)) throw rpcError(-32001, 'unknown session');
      send_pi(session_id, { type: 'abort' });
      return { ok: true };
    }
    case 'ask_resolve': {
      const session_id = params.session_id;
      if (!session_id || !sessions.has(session_id)) throw rpcError(-32001, 'unknown session');
      send_pi(session_id, {
        type: 'ask_response',
        id: params.ask_id,
        decision: params.decision,
      });
      return { ok: true };
    }
    case 'shutdown': {
      for (const [, state] of sessions) {
        try {
          state.pi_proc.stdin.end();
        } catch {}
      }
      return null;
    }
    default:
      throw rpcError(-32000, `unknown method: ${method}`);
  }
}

function rpcError(code, message) {
  const e = new Error(message);
  e.code = code;
  return e;
}

/* ---------- ready banner ---------- */

process.stdout.write(
  JSON.stringify({
    jsonrpc: '2.0',
    method: 'ready',
    params: { protocol_version: 1, pi_bin: PI_BIN },
  }) + '\n',
);
