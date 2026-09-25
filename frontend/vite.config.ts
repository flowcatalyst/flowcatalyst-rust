import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import Components from "unplugin-vue-components/vite";
import { PrimeVueResolver } from "@primevue/auto-import-resolver";
import { fileURLToPath, URL } from "node:url";

// Rust backend port (fc-dev default: 8080)
const BACKEND_PORT = process.env.VITE_BACKEND_PORT ?? "8080";
const BACKEND_URL = `http://localhost:${BACKEND_PORT}`;

export default defineConfig({
	plugins: [
		vue(),
		Components({
			resolvers: [PrimeVueResolver()],
		}),
	],
	appType: "spa",
	resolve: {
		alias: {
			"@": fileURLToPath(new URL("./src", import.meta.url)),
		},
	},
	server: {
		port: 4200,
		proxy: {
			"/api": {
				target: BACKEND_URL,
				changeOrigin: true,
			},
			"/bff": {
				target: BACKEND_URL,
				changeOrigin: true,
			},
			"/auth": {
				target: BACKEND_URL,
				changeOrigin: true,
				// Bypass: let the SPA handle browser navigation requests
				// (HTML pages), only proxy API/XHR calls to the backend
				bypass(req) {
					const accept = req.headers.accept ?? "";
					if (
						accept.includes("text/html") &&
						!req.url?.startsWith("/auth/oidc/callback")
					) {
						return req.url;
					}
				},
			},
			"/oauth": {
				target: BACKEND_URL,
				changeOrigin: true,
			},
			"/.well-known": {
				target: BACKEND_URL,
				changeOrigin: true,
			},
		},
	},
});
