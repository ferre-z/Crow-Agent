import path from "node:path";

import { app, BrowserWindow, ipcMain, Menu, Notification } from "electron";

import type {
  ApprovalRespondRequest,
  CreateSessionRequest,
  SendPromptRequest,
} from "../shared/api.ts";
import type { KnownHost } from "../shared/hosts.ts";
import { ConnectionManager } from "./connection-manager.ts";
import { loadHosts, removeHost, saveHosts, upsertHost } from "./hosts-store.ts";

/**
 * Crow desktop main process. P3: multihost fleet. The ConnectionManager holds
 * one CrowClient per connected daemon and forwards events/state to the renderer
 * stamped with the host name.
 */

let mainWindow: BrowserWindow | undefined;
const hostsFile = (): string => path.join(app.getPath("userData"), "hosts.json");

function isFocused(): boolean {
  return mainWindow?.isFocused() ?? false;
}

function broadcast(channel: "daemon:event" | "daemon:state", payload: unknown): void {
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send(channel, payload);
  }
}

function expandHome(cwd: string, url: string): string {
  if (!isLoopbackUrl(url)) return cwd;
  if (cwd === "~") return app.getPath("home");
  if (cwd.startsWith("~/")) return path.join(app.getPath("home"), cwd.slice(2));
  return cwd;
}

function isLoopbackUrl(url: string): boolean {
  try {
    const hostname = new URL(url).hostname;
    return hostname === "localhost" || hostname === "127.0.0.1" || hostname === "::1";
  } catch {
    return false;
  }
}

const manager = new ConnectionManager({
  onEvent: (hostName, method, params) => broadcast("daemon:event", { hostName, method, params }),
  onStateChange: (hostName, state) => broadcast("daemon:state", { hostName, state }),
  Notification,
  isFocused,
});

function registerIpc(): void {
  ipcMain.handle("hosts:list", () => loadHosts(hostsFile()));

  ipcMain.handle("hosts:add", async (_event, host: KnownHost) => {
    const next = upsertHost(await loadHosts(hostsFile()), host);
    await saveHosts(hostsFile(), next);
    return next;
  });

  ipcMain.handle("hosts:remove", async (_event, name: string) => {
    await manager.disconnect(name);
    const next = removeHost(await loadHosts(hostsFile()), name);
    await saveHosts(hostsFile(), next);
    return next;
  });

  ipcMain.handle("host:connect", (_event, host: KnownHost) => manager.add(host));

  ipcMain.handle("host:disconnect", (_event, hostName: string) => manager.disconnect(hostName));

  ipcMain.handle("host:reconnect", (_event, hostName: string) => {
    const conn = manager.get(hostName);
    if (!conn) throw new Error(`unknown host: ${hostName}`);
    return manager.connect(conn.host);
  });

  ipcMain.handle("fleet:list", () => manager.list());

  ipcMain.handle("session:create", async (_event, params: CreateSessionRequest) => {
    const conn = manager.get(params.hostName);
    if (!conn) throw new Error(`unknown host: ${params.hostName}`);
    return manager.createSession(params.hostName, {
      ...params,
      cwd: expandHome(params.cwd, conn.host.url),
    });
  });

  ipcMain.handle("session:send", (_event, params: SendPromptRequest) =>
    manager.sendPrompt(params.hostName, params.sessionId, params.text),
  );

  ipcMain.handle("session:cancel", (_event, { hostName, sessionId }) =>
    manager.cancelSession(hostName, sessionId),
  );

  ipcMain.handle("session:list", (_event, hostName: string) => manager.listSessions(hostName));

  ipcMain.handle("session:attach", (_event, { hostName, sessionId }) =>
    manager.attachSession(hostName, sessionId),
  );

  ipcMain.handle("approval:respond", (_event, params: ApprovalRespondRequest) =>
    manager.respondApproval(params.hostName, params.approvalId, params.decision),
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
  if (!app.isPackaged) Menu.setApplicationMenu(null);
  registerIpc();
  await createWindow();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) void createWindow();
  });
});

app.on("window-all-closed", () => {
  void manager.closeAll().finally(() => {
    if (process.platform !== "darwin") app.quit();
  });
});
