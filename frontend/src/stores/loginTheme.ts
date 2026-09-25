import { defineStore } from "pinia";
import { ref, computed } from "vue";

export interface LoginTheme {
	brandName: string;
	brandSubtitle: string;
	logoUrl?: string;
	logoSvg?: string;
	logoHeight?: number;
	primaryColor: string;
	accentColor: string;
	backgroundColor: string;
	backgroundGradient?: string;
	footerText: string;
	customCss?: string;
}

const DEFAULT_THEME: LoginTheme = {
	brandName: "FlowCatalyst",
	brandSubtitle: "Platform Administration",
	logoHeight: 40,
	primaryColor: "#102a43",
	accentColor: "#0967d2",
	backgroundColor: "#0a1929",
	backgroundGradient: "linear-gradient(135deg, #102a43 0%, #0a1929 100%)",
	footerText: "Secure access to your FlowCatalyst platform",
};

// The tenant client whose branding the login surface should wear. A
// relying party names it via /oauth/authorize?client=<identifier>, which
// forwards it to /auth/login. It is remembered for the tab's lifetime so
// the forgot-password and reset pages — reached by links that carry no
// query string — stay branded.
const CLIENT_STORAGE_KEY = "fc_login_client";

/**
 * Normalizes a `client` route-query value. vue-router types a query entry
 * as `string | null` or an array of those, so a repeated param collapses
 * to its first usable entry.
 */
export function normalizeClientParam(
	value: string | null | undefined | (string | null)[],
): string | undefined {
	const raw = Array.isArray(value) ? value[0] : value;
	const trimmed = typeof raw === "string" ? raw.trim() : "";
	return trimmed === "" ? undefined : trimmed;
}

function rememberClient(identifier?: string): string | undefined {
	try {
		if (identifier) {
			sessionStorage.setItem(CLIENT_STORAGE_KEY, identifier);
			return identifier;
		}
		return sessionStorage.getItem(CLIENT_STORAGE_KEY) || undefined;
	} catch {
		// Private-browsing / disabled storage — branding is best-effort.
		return identifier;
	}
}

export const useLoginThemeStore = defineStore("loginTheme", () => {
	// State
	const theme = ref<LoginTheme>(DEFAULT_THEME);
	const isLoaded = ref(false);
	const error = ref<string | null>(null);
	// The client the currently-held theme was fetched for. Keying the
	// cache on it (rather than a bare isLoaded flag) lets a second page
	// re-fetch when it names a different client, while still collapsing
	// repeat loads for the same one.
	const loadedClient = ref<string | undefined>(undefined);

	// Computed
	const hasCustomLogo = computed(() =>
		Boolean(theme.value.logoUrl || theme.value.logoSvg),
	);

	const background = computed(
		() => theme.value.backgroundGradient || theme.value.backgroundColor,
	);

	// Actions

	/**
	 * Loads the login theme, optionally branded for a tenant client.
	 *
	 * `clientIdentifier` is the client's URL-safe slug, taken from the
	 * `client` query param. Omitting it falls back to the one remembered
	 * earlier in this tab, then to the platform-wide theme. The backend
	 * layers the client's theme over the platform one field by field, so
	 * anything the client hasn't overridden still shows through.
	 */
	async function loadTheme(clientIdentifier?: string): Promise<void> {
		const client = rememberClient(clientIdentifier);
		if (isLoaded.value && loadedClient.value === client) return;

		try {
			const url = client
				? `/api/public/login-theme?client=${encodeURIComponent(client)}`
				: "/api/public/login-theme";

			const response = await fetch(url);
			if (response.ok) {
				const data = await response.json();
				theme.value = { ...DEFAULT_THEME, ...data };
			} else {
				console.warn("Failed to load login theme, using defaults");
			}
		} catch (err) {
			console.warn("Failed to load login theme, using defaults:", err);
			error.value = err instanceof Error ? err.message : "Unknown error";
		} finally {
			isLoaded.value = true;
			loadedClient.value = client;
		}
	}

	function applyThemeColors(): void {
		const root = document.documentElement;
		root.style.setProperty("--login-primary", theme.value.primaryColor);
		root.style.setProperty("--login-accent", theme.value.accentColor);
		root.style.setProperty("--login-bg", theme.value.backgroundColor);

		// Apply custom CSS if provided
		if (theme.value.customCss) {
			let styleEl = document.getElementById("login-custom-css");
			if (!styleEl) {
				styleEl = document.createElement("style");
				styleEl.id = "login-custom-css";
				document.head.appendChild(styleEl);
			}
			styleEl.textContent = theme.value.customCss;
		}
	}

	function reset(): void {
		theme.value = DEFAULT_THEME;
		isLoaded.value = false;
		loadedClient.value = undefined;
		error.value = null;

		// Remove custom CSS
		const styleEl = document.getElementById("login-custom-css");
		if (styleEl) {
			styleEl.remove();
		}
	}

	return {
		// State
		theme,
		isLoaded,
		loadedClient,
		error,
		// Computed
		hasCustomLogo,
		background,
		// Actions
		loadTheme,
		applyThemeColors,
		reset,
	};
});
