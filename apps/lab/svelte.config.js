// Tauri has no Node server, so there is no SSR: adapter-static with an
// index.html fallback puts SvelteKit in SPA mode.
// https://svelte.dev/docs/kit/single-page-apps
import adapter from '@sveltejs/adapter-static';
import { relative, sep } from 'node:path';

/** @type {import('@sveltejs/kit').Config} */
const config = {
	compilerOptions: {
		// Force runes mode for our own code, leave node_modules on the library's
		// own setting (some shadcn deps still ship legacy components).
		runes: ({ filename }) => {
			const relativePath = relative(import.meta.dirname, filename);
			const pathSegments = relativePath.toLowerCase().split(sep);
			const isExternalLibrary = pathSegments.includes('node_modules');
			return isExternalLibrary ? undefined : true;
		}
	},
	kit: {
		adapter: adapter({
			fallback: 'index.html'
		})
	}
};

export default config;
