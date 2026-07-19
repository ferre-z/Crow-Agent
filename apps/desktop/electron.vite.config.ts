import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "electron-vite";

// externalizeDeps: false — bundle workspace deps (@crow/client ships TS
// sources; electron's runtime could not import them) plus ws/zod into
// self-contained main/preload bundles.
export default defineConfig({
  main: {
    build: { externalizeDeps: false },
  },
  preload: {
    build: { externalizeDeps: false },
  },
  renderer: {
    plugins: [react(), tailwindcss()],
  },
});
