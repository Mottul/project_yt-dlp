import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Tauri lädt die Oberfläche im Entwicklungsmodus von diesem Server und im
// Paket aus ../dist. Fester Port, damit tauri.conf.json darauf zeigen kann.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5183,
    strictPort: true,
    // Nur die Oberfläche beobachten. In target/ sperrt Cargo beim Bauen Dateien;
    // Vite stürzte dann mit EBUSY ab (8.11.2026, während `cargo clippy`) – bei
    // `npm run app:dev` bauen beide gleichzeitig.
    watch: { ignored: ['**/target/**', '**/src-tauri/**', '**/core/**', '**/cli/**'] }
  },
  build: { target: 'es2022', sourcemap: false }
})
