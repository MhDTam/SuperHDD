import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
export default defineConfig({
  plugins: [react()],
  root: "apps/desktop",
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { outDir: "../../dist", emptyOutDir: true },
  envPrefix: ["VITE_", "TAURI_"],
});
