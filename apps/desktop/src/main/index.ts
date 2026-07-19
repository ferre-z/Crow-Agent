import path from "node:path";

import { CrowClient, CrowClientError } from "@crow/client";
import { app, BrowserWindow, ipcMain, Menu } from "electron";

import type {
  ApprovalRespondRequest,
  ConnectResult,
  CreateSessionRequest,
  SendPromptRequest,
} from "../shared/api.ts";
import type { KnownHost } from "../shared/hosts.ts";
import { loadHosts, removeHost, saveHosts, upsertHost } from "./hosts-store.ts";

/**
 * Crow desktop main process. P2: one window, ONE daemon connection at a time
 * (multihost fan-out is P3 — see the report/notes in docs). All daemon events
 * are forwarded to the renderer as "daemon:event"; connection state changes as
 * "daemon:state".
 */

let mainWindow: BrowserWindow | undefined;
let client: CrowClient | undefined;
/** URL of the currently connected host — kept for loopback-aware cwd expansion. */
let connectedUrl: string | undefined;

const hostsFile = (): string => path.join(app.getPath("userData"), "hosts.json");

function broadcast(channel: "daemon:event" | "daemon:state", payload: unknown): void {
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send(channel, payload);
  }
}

function wireClient(next: CrowClient): void {
  next.onEvent((method, params) => broadcast("daemon:event", { method, params }));
  next.onStateChange((state) => broadcast("daemon:state", state));
}

async function disconnectClient(): Promise<void> {
  const current = client;
  client = undefined;
  connectedUrl = undefined;
  if (current) await current.close();
}

function requireClient(): CrowClient {
  if (!client) throw new Error("not connected to a daemon");
  return client;
}

/**
 * Expand a leading "~" against the local home dir. Only meaningful when the
 * daemon is on this machine (the P2 norm); for remote daemons the path is sent
 * verbatim and the daemon interprets it (P3 moves expansion daemon-side).
 */
function expandHome(cwd: string): string {
  if (!isLoopbackUrl(connectedUrl)) return cwd;
  if (cwd === "~") return app.getPath("home");
  if (cwd.startsWith("~/")) return path.join(app.getPath("home"), cwd.slice(2));
  return cwd;
}

function isLoopbackUrl(url: string | undefined): boolean {
  if (!url) return false;
  try {
    const hostname = new URL(url).hostname;
    return hostname === "localhost" || hostname === "127.0.0.1" || hostname === "::1";
  } catch {
    return false;
  }
}

function classifyConnectError(error: unknown): ConnectResult {
  const message = error instanceof Error ? error.message : String(error);
  if (error instanceof CrowClientError && message.includes("HTTP 401")) {
    return { ok: false, kind: "auth", message };
  }
  if (/refused|host not found|timed out/.test(message)) {
    return { ok: false, kind: "unreachable", message };
  }
  return { ok: false, kind: "error", message };
}

function registerIpc(): void {
  ipcMain.handle("hosts:list", () => loadHosts(hostsFile()));

  ipcMain.handle("hosts:add", async (_event, host: KnownHost) => {
    const next = upsertHost(await loadHosts(hostsFile()), host);
    await saveHosts(hostsFile(), next);
    return next;
  });

  ipcMain.handle("hosts:remove", async (_event, name: string) => {
    const next = removeHost(await loadHosts(hostsFile()), name);
    await saveHosts(hostsFile(), next);
    return next;
  });

  ipcMain.handle("host:connect", async (_event, host: KnownHost): Promise<ConnectResult> => {
    await disconnectClient();
    const next = new CrowClient({ url: host.url, token: host.token });
    wireClient(next);
    try {
      await next.connect();
      const info = await next.hostInfo();
      client = next;
      connectedUrl = host.url;
      return { ok: true, info };
    } catch (error) {
      await next.close().catch(() => undefined);
      return classifyConnectError(error);
    }
  });

  ipcMain.handle("host:disconnect", () => disconnectClient());

  ipcMain.handle("session:create", (_event, params: CreateSessionRequest) =>
    requireClient().createSession({ ...params, cwd: expandHome(params.cwd) }),
  );

  ipcMain.handle("session:send", (_event, { sessionId, text }: SendPromptRequest) =>
    requireClient().sendPrompt(sessionId, text),
  );

  ipcMain.handle("session:cancel", (_event, sessionId: string) =>
    requireClient().cancelSession(sessionId),
  );

  ipcMain.handle("session:list", () => requireClient().listSessions());

  ipcMain.handle("session:attach", (_event, sessionId: string) =>
    requireClient().attachSession(sessionId),
  );

  ipcMain.handle("approval:respond", (_event, { approvalId, decision }: ApprovalRespondRequest) =>
    requireClient().respondApproval(approvalId, decision),
  );
}

async function createWindow(): Promise<void> {
  mainWindow = new BrowserWindow({
    width: 1440,
    height: 900,
    backgroundColor: "#0a0f0a",
    autoHideMenuBar: true,
    webPreferences: {
      preload: path.join(import.meta.dirname, "../preload/index.mjs"),
      // ESM preload (.mjs) requires an unsandboxed renderer; context isolation
      // and no node integration still apply.
      sandbox: false,
      contextIsolation: true,
      nodeIntegration: false,
    },
  });
  mainWindow.on("closed", () => {
    mainWindow = undefined;
  });

  const devUrl = process.env.ELECTRON_RENDERER_URL;
  if (devUrl) await mainWindow.loadURL(devUrl);
  else await mainWindow.loadFile(path.join(import.meta.dirname, "../renderer/index.html"));
}

app.whenReady().then(async () => {
  if (!app.isPackaged) Menu.setApplicationMenu(null); // P2 dev build: no menu bar
  registerIpc();
  await createWindow();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) void createWindow();
  });
});

app.on("window-all-closed", () => {
  void disconnectClient().finally(() => {
    if (process.platform !== "darwin") app.quit();
  });
});
