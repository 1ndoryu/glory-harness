import { defineConfig } from 'vite';

export default defineConfig({
  server: {
    port: 8760,
    strictPort: true,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:8799',
        changeOrigin: true,
      },
    },
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
