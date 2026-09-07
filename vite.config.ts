import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Tauri lädt die Oberfläche im Entwicklungsmodus von diesem Server und im
// Paket aus ../dist. Fester Port, damit tauri.conf.json darauf zeigen kann.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5183, strictPort: true },
  build: { target: 'es2022', sourcemap: false }
})
