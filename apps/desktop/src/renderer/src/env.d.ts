/// <reference types="vite/client" />
import type { CrowBridge } from "../../shared/api.ts";

declare global {
  interface Window {
    crow: CrowBridge;
  }
}

export {};
