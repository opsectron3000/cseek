import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

const target = 'http://127.0.0.1:5030';

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      '/api': { target, changeOrigin: true, ws: true },
      '/metrics': { target, changeOrigin: true },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    sourcemap: false,
    // Keep flag SVGs as separate files fetched on demand instead of inlining ~500 into the CSS.
    assetsInlineLimit: (file) => (file.includes('flag-icons') ? false : undefined),
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('node_modules')) return id.includes('@base-ui') || id.includes('@floating-ui') ? 'base-ui' : 'react';
        },
      },
    },
  },
});
