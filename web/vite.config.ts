import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import wasm from "vite-plugin-wasm";
import cesium from "vite-plugin-cesium";

export default defineConfig({
  base: "/trajix/",
  plugins: [react(), wasm(), cesium()],
  worker: {
    plugins: () => [wasm()],
    format: "es",
  },
  optimizeDeps: {
    // Don't pre-bundle trajix-wasm: let wasm-pack's init()
    // resolve trajix_wasm_bg.wasm relative to its own JS file.
    exclude: ["trajix-wasm"],
  },
});
