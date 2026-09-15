import { defineConfig } from 'vite';

export default defineConfig({
  preview: { port: 17038, strictPort: true },
  server: {
    strictPort: true,
    host: '127.0.0.1',
    port: 16150,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:7788',
        ws: true,
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
});
