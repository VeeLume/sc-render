import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

// Tauri sets TAURI_DEV_HOST to the machine's LAN IP when targeting a real
// device, so the dev server must bind that interface (not just localhost).
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
	plugins: [
		tailwindcss(),
		sveltekit()
	],
	// Don't let Vite wipe the terminal — it hides Rust compile errors.
	clearScreen: false,
	server: {
		host: host || false,
		port: 1420,
		strictPort: true,
		hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
		watch: {
			// Rust rebuilds are handled by Tauri, not Vite.
			ignored: ['**/src-tauri/**']
		}
	}
});
