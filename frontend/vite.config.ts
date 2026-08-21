import { defineConfig } from 'vite';

export default defineConfig({
  server: {
    port: 4647,
    strictPort: true,
    proxy: {
      // 로컬 개발: backend(:4646)로 API 프록시
      '/api': 'http://localhost:4646',
    },
  },
  build: {
    target: 'es2022',
  },
});
