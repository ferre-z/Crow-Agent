import { useEffect, useRef, useState, type Dispatch } from "react";

import {
  sessionDisplayName,
  type Action,
  type SessionEntry,
  type TranscriptItem,
} from "./state.ts";

function pretty(value: unknown): string {
  if (value === undefined) return "";
  try {
    return JSON.stringify(value, null, 2) ?? String(value);
  } catch {
    return String(value);
  }
}

function ToolCard({ item }: { item: Extract<TranscriptItem, { kind: "tool" }> }) {
  return (
    <div
      className={`rounded border bg-ink-1 text-xs ${
        item.done && item.isError ? "border-danger/60" : "border-line"
      }`}
    >
      <div className="flex items-center gap-2 border-b border-line px-2 py-1">
        <span className="font-mono font-semibold text-crow">{item.tool}</span>
        <span className="text-fg-dim">
          {item.done ? (item.isError ? "failed" : "done") : "running…"}
        </span>
      </div>
      {item.args !== undefined ? (
        <pre className="max-h-40 overflow-auto px-2 py-1 text-fg-dim">{pretty(item.args)}</pre>
      ) : null}
      {item.done ? (
        <pre
          className={`max-h-60 overflow-auto border-t border-line px-2 py-1 ${
            item.isError ? "text-danger" : "text-fg"
          }`}
        >
          {item.output || "(no output)"}
        </pre>
      ) : null}
    </div>
  );
}

function TranscriptRow({ item }: { item: TranscriptItem }) {
  switch (item.kind) {
    case "user":
      return (
        <div className="flex justify-end">
          <div className="max-w-[75%] whitespace-pre-wrap rounded-lg bg-crow/15 px-3 py-1.5 text-sm">
            {item.text}
          </div>
        </div>
      );
    case "assistant":
      return <div className="whitespace-pre-wrap text-sm leading-relaxed">{item.text}</div>;
    case "thinking":
      return (
        <details className="rounded border border-line bg-ink-1 px-2 py-1 text-xs text-fg-dim">
          <summary className="cursor-pointer select-none italic">thinking…</summary>
          <div className="mt-1 whitespace-pre-wrap italic">{item.text}</div>
        </details>
      );
    case "tool":
      return <ToolCard item={item} />;
    case "approval":
      return (
        <div className="rounded border border-warn/40 bg-warn/5 px-2 py-1 text-xs">
          <span className="font-semibold text-warn">approval</span>{" "}
          <span className="font-mono">{item.tool}</span>{" "}
          <span className="text-fg-dim">
            {item.decision ? `→ ${item.decision}` : "→ awaiting response"}
          </span>
        </div>
      );
  }
}

export default function ChatView({
  session,
  dispatch,
}: {
  session: SessionEntry;
  dispatch: Dispatch<Action>;
}) {
  const [input, setInput] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);
  const sessionId = session.info.id;
  const streaming = session.live === "streaming";

  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [session.transcript.length]);

  function send() {
    const text = input.trim();
    if (!text || streaming) return;
    dispatch({ type: "prompt.sent", sessionId, text });
    void window.crow.sessionSend({ sessionId, text }).catch(() => undefined);
    setInput("");
  }

  function cancel() {
    void window.crow.sessionCancel(sessionId).catch(() => undefined);
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex items-center gap-3 border-b border-line px-4 py-2">
        <span className="text-sm font-semibold">{sessionDisplayName(session)}</span>
        <span className="font-mono text-xs text-fg-dim">{session.info.cwd}</span>
        <span
          className={`ml-auto rounded-full px-2 py-0.5 text-xs ${
            session.live === "error"
              ? "bg-danger/15 text-danger"
              : session.live === "cancelled"
                ? "bg-warn/15 text-warn"
                : session.live === "streaming"
                  ? "bg-crow/15 text-crow"
                  : "bg-line text-fg-dim"
          }`}
        >
          {session.live}
        </span>
      </header>

      {session.live === "error" && session.error ? (
        <div className="border-b border-danger/40 bg-danger/10 px-4 py-2 text-xs text-danger">
          {session.error}
        </div>
      ) : null}

      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        <div className="mx-auto flex max-w-3xl flex-col gap-3">
          {session.transcript.length === 0 ? (
            <p className="py-8 text-center text-sm text-fg-dim">
              Send a prompt to start working in this session.
            </p>
          ) : (
            session.transcript.map((item) => <TranscriptRow key={item.id} item={item} />)
          )}
        </div>
      </div>

      <footer className="border-t border-line p-3">
        <div className="mx-auto flex max-w-3xl items-end gap-2">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                send();
              }
            }}
            placeholder={
              streaming
                ? "session is streaming…"
                : "prompt  (Enter to send, Shift+Enter for newline)"
            }
            rows={2}
            className="min-w-0 flex-1 resize-none rounded border border-line bg-ink-1 px-3 py-2 text-sm outline-none focus:border-crow"
          />
          {streaming ? (
            <button
              onClick={cancel}
              className="rounded border border-danger px-3 py-2 text-sm text-danger hover:bg-danger/10"
            >
              cancel
            </button>
          ) : (
            <button
              onClick={send}
              disabled={!input.trim()}
              className="rounded bg-crow px-4 py-2 text-sm font-semibold text-ink hover:bg-crow-dim disabled:opacity-50"
            >
              send
            </button>
          )}
        </div>
      </footer>
    </div>
  );
}
