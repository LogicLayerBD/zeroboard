import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

// Backend address used only by the dev server proxy; production serves the frontend from the binary.
const BACKEND_URL = 'http://localhost:3000';
const DEV_PORT = 5173;

export default defineConfig({
	plugins: [sveltekit()],
	server: {
		port: DEV_PORT,
		strictPort: true,
		proxy: {
			'/api': BACKEND_URL,
			'/ws': { target: BACKEND_URL, ws: true }
		}
	}
});
