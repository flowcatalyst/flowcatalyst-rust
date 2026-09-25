import { apiFetch } from "./client";
import type {
	AccessListResponse as GenAccessListResponse,
	ConfigListResponse as GenConfigListResponse,
	ConfigResponse,
} from "./generated";

// Hand-rolled by design: GET /api/public/login-theme is a chi-mounted public
// route outside huma, so it has no generated type (same as the BFF/auth
// surfaces — see docs/frontend-api-types-adoption.md "What NOT to migrate").
export interface LoginTheme {
	brandName: string;
	brandSubtitle: string;
	logoUrl?: string | null;
	logoSvg?: string | null;
	logoHeight?: number;
	primaryColor: string;
	accentColor: string;
	backgroundColor: string;
	backgroundGradient?: string | null;
	footerText: string;
	customCss?: string | null;
}

/**
 * The built-in platform look, used to seed a fresh theme form. Returns a
 * new object each call so callers can edit it freely.
 */
export function defaultLoginTheme(): LoginTheme {
	return {
		brandName: "FlowCatalyst",
		brandSubtitle: "Platform Administration",
		logoUrl: null,
		logoSvg: null,
		logoHeight: 40,
		primaryColor: "#102a43",
		accentColor: "#0967d2",
		backgroundColor: "#0a1929",
		backgroundGradient: "linear-gradient(135deg, #102a43 0%, #0a1929 100%)",
		footerText: "Secure access to your FlowCatalyst platform",
		customCss: null,
	};
}

// Response types alias the generated contract (api/openapi.lock.json) so
// `vue-tsc` fails on backend drift. Aliased under the historical names so
// pages keep their imports. (`clientId`/`description` are optional on the
// wire, not `| null` as the old hand-rolled type claimed.)
export type PlatformConfig = ConfigResponse;
// Wire types for the platform-config admin endpoints
// (GET /platform-config/{app} and GET /platform-config/{app}/access);
// the SPA has no callers yet, the aliases are here for when it grows them.
// The grant body on POST /platform-config/{app}/access is
// `{ roleCode, canWrite }` (GrantAccessRequest); revoke returns 204.
export type ConfigListResponse = GenConfigListResponse;
export type AccessListResponse = GenAccessListResponse;

export interface SetConfigRequest {
	value: string;
	valueType?: "PLAIN" | "SECRET";
	description?: string;
}

// NOTE: the old module sent a `scope` query param on every call — the wire
// has no such parameter; the backend derives the scope from `clientId`
// (absent = GLOBAL, present = CLIENT). Dropped when adopting the generated
// types.
function configUrl(
	appCode: string,
	section: string,
	property: string,
	clientId?: string,
): string {
	let url = `/config/${appCode}/${section}/${property}`;
	if (clientId) url += `?clientId=${encodeURIComponent(clientId)}`;
	return url;
}

export const configApi = {
	// Get login theme (public, no auth needed)
	getLoginTheme(clientId?: string): Promise<LoginTheme> {
		const url = clientId
			? `/public/login-theme?clientId=${encodeURIComponent(clientId)}`
			: "/public/login-theme";
		return apiFetch(url);
	},

	// Get a config value
	getConfig(
		appCode: string,
		section: string,
		property: string,
		clientId?: string,
	): Promise<PlatformConfig> {
		return apiFetch(configUrl(appCode, section, property, clientId));
	},

	// Set a config value
	setConfig(
		appCode: string,
		section: string,
		property: string,
		data: SetConfigRequest,
		clientId?: string,
	): Promise<PlatformConfig> {
		return apiFetch(configUrl(appCode, section, property, clientId), {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	// Delete a config
	deleteConfig(
		appCode: string,
		section: string,
		property: string,
		clientId?: string,
	): Promise<void> {
		return apiFetch(configUrl(appCode, section, property, clientId), {
			method: "DELETE",
		});
	},

	// Helpers specifically for the login theme. Passing a clientId targets
	// that client's CLIENT-scoped override; omitting it targets the
	// platform-wide GLOBAL row. The public endpoint layers the former over
	// the latter, so a client only needs to store what it changes.
	// No stored theme is the normal case — a client that has never been
	// branded, or a fresh platform — and the backend answers 404 for it. That
	// is an answer, not a failure, so suppress the global error banner and let
	// the `null` fallback below stand in silently.
	getLoginThemeConfig(clientId?: string): Promise<string | null> {
		return apiFetch<PlatformConfig>(
			configUrl("platform", "login", "theme", clientId),
			{ suppressGlobalErrorToast: true },
		)
			.then((response: PlatformConfig) => response.value)
			.catch(() => null);
	},

	setLoginThemeConfig(
		theme: LoginTheme,
		clientId?: string,
	): Promise<PlatformConfig> {
		return apiFetch(configUrl("platform", "login", "theme", clientId), {
			method: "PUT",
			body: JSON.stringify({
				value: JSON.stringify(theme),
				valueType: "PLAIN",
				description: clientId
					? "Client login page theme configuration"
					: "Login page theme configuration",
			}),
		});
	},

	// Drops a client's override so its login page falls back to the
	// platform theme. A 404 means there was nothing to clear.
	clearLoginThemeConfig(clientId: string): Promise<void> {
		return apiFetch<void>(configUrl("platform", "login", "theme", clientId), {
			method: "DELETE",
			suppressGlobalErrorToast: true,
		}).catch(() => undefined);
	},

	// Platform name — the brand shown in emails, the authenticator app (2FA
	// issuer), passkey prompts, and the SPA. Stored at platform/branding/platform-name.
	getPlatformName(): Promise<string | null> {
		return apiFetch<PlatformConfig>("/config/platform/branding/platform-name", {
			suppressGlobalErrorToast: true,
		})
			.then((response: PlatformConfig) => response.value)
			.catch(() => null);
	},

	setPlatformName(name: string): Promise<PlatformConfig> {
		return apiFetch("/config/platform/branding/platform-name", {
			method: "PUT",
			body: JSON.stringify({
				value: name,
				valueType: "PLAIN",
				description: "Platform display name",
			}),
		});
	},
};
