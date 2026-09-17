import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

/**
 * Everything Tauri needs is already here, so wrapping this app in a desktop shell
 * is purely additive — the Tauri commit adds `src-tauri/` and changes nothing in
 * this file.
 *
 *   base: './'        — dist/ must work from tauri:// and file://, not just a server root
 *   clearScreen       — leave the Tauri CLI's output on screen
 *   strictPort        — tauri.conf.json hardcodes 5173; silently moving to 5174 would
 *                       leave the desktop window pointed at nothing
 */
export default defineConfig({
  plugins: [react()],
  base: './',
  clearScreen: false,
  envPrefix: ['VITE_', 'TAURI_'],
  server: { port: 5173, strictPort: true, host: '127.0.0.1' },
  build: { outDir: 'dist', target: 'esnext', emptyOutDir: true },
  // The workspace packages publish TypeScript source rather than a build output.
  // Vite handles that, but esbuild's dependency pre-bundling must not try to.
  optimizeDeps: {
    exclude: ['@duckfoot/core', '@duckfoot/media', '@duckfoot/ui', '@duckfoot/tool-photo', '@duckfoot/tool-audio'],
  },
});
