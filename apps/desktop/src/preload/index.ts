import { contextBridge, ipcRenderer, type IpcRendererEvent } from "electron";

import type { CrowBridge } from "../shared/api.ts";

/**
 * The renderer's only door into the main process: a typed `window.crow`
 * bridge over exactly the Crow IPC channels. Raw ipcRenderer is never exposed.
 */
function subscribe<T>(channel: string, listener: (payload: T) => void): () => void {
  const wrapped = (_event: IpcRendererEvent, payload: T) => listener(payload);
  ipcRenderer.on(channel, wrapped);
  return () => {
    ipcRenderer.removeListener(channel, wrapped);
  };
}

const bridge: CrowBridge = {
  hostsList: () => ipcRenderer.invoke("hosts:list"),
  hostsAdd: (host) => ipcRenderer.invoke("hosts:add", host),
  hostsRemove: (name) => ipcRenderer.invoke("hosts:remove", name),
  hostConnect: (host) => ipcRenderer.invoke("host:connect", host),
  hostDisconnect: () => ipcRenderer.invoke("host:disconnect"),
  sessionCreate: (params) => ipcRenderer.invoke("session:create", params),
  sessionSend: (params) => ipcRenderer.invoke("session:send", params),
  sessionCancel: (sessionId) => ipcRenderer.invoke("session:cancel", sessionId),
  sessionList: () => ipcRenderer.invoke("session:list"),
  sessionAttach: (sessionId) => ipcRenderer.invoke("session:attach", sessionId),
  approvalRespond: (params) => ipcRenderer.invoke("approval:respond", params),
  onDaemonEvent: (listener) => subscribe("daemon:event", listener),
  onDaemonState: (listener) => subscribe("daemon:state", listener),
};

contextBridge.exposeInMainWorld("crow", bridge);
