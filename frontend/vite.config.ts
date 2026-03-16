import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    proxy: {
      '/dashboard': 'http://localhost:8317',
      '/v0': 'http://localhost:8317',
      '/v1': 'http://localhost:8317',
      '/v1beta': 'http://localhost:8317',
      '/api': 'http://localhost:8317',
      '/health': 'http://localhost:8317',
    },
  },
})
