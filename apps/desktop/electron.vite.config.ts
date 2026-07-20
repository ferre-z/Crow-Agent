import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "electron-vite";

// externalizeDeps: false — bundle workspace deps (@crow/client ships TS
// sources; electron's runtime could not import them) plus ws/zod into
// self-contained main/preload bundles. ws's optional native addons
// (bufferutil, utf-8-validate) are aliased to a stub: they are try/catch
// performance hints in ws, but the bundler makes them hard ESM imports.
const wsOptionalStub = new URL("./src/main/ws-optional-stub.ts", import.meta.url).pathname;

export default defineConfig({
  main: {
    build: { externalizeDeps: false },
    resolve: {
      alias: {
        bufferutil: wsOptionalStub,
        "utf-8-validate": wsOptionalStub,
      },
    },
  },
  preload: {
    build: { externalizeDeps: false },
  },
  renderer: {
    plugins: [react(), tailwindcss()],
  },
});
