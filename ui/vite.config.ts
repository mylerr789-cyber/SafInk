import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { readFileSync, existsSync } from 'node:fs';
import { resolve, basename } from 'node:path';

// Yalnızca `vite` geliştirme sunucusu: tarayıcıda Tauri olmadan denemek için örnek sayfalar (derlemeye girmez).
const mockFiles = {
  name: 'safink-mock-files',
  apply: 'serve' as const,
  configureServer(server: any) {
    server.middlewares.use('/__mock/', (req: any, res: any, next: any) => {
      const name = basename(decodeURIComponent((req.url || '').split('?')[0]));
      const f = name === 'catalog.json' ? resolve(__dirname, '../crates/safink-core/catalog.json') : resolve(__dirname, 'mock', name);
      if (!existsSync(f)) return next();
      res.setHeader('Content-Type', f.endsWith('.json') ? 'application/json' : f.endsWith('.png') ? 'image/png' : 'image/jpeg');
      res.end(readFileSync(f));
    });
  },
};

export default defineConfig({
  plugins: [tailwindcss(), svelte(), mockFiles],
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: '127.0.0.1' },
  build: { target: 'esnext', chunkSizeWarningLimit: 2500 },
});
