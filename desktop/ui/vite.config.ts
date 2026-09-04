import { defineConfig } from 'vite';

export default defineConfig({
  // Puerto fijo para poder abrir el front sin backend (mismo puerto de
  // desarrollo que usaba el boceto del plan 039A-1).
  server: {
    port: 8760,
    strictPort: true,
  },
  preview: {
    port: 8760,
    strictPort: true,
  },
  build: {
    outDir: 'dist',
    assetsDir: 'assets',
    target: 'es2022',
  },
});
