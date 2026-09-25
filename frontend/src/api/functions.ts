import { apiFetch, type FetchOptions } from "./client";
import type {
	AliasResponse,
	CheckManifestRequest,
	CheckManifestResponse,
	ClaimRequest,
	ConfigResponse,
	CreateFunctionRequest,
	DomainResponse,
	FunctionPageResponse,
	FunctionResponse,
	FunctionRouteResponse,
	Manifest,
	ManifestErrorResponse,
	PolicyListResponse,
	PolicyResponse,
	PolicySignerRequest,
	PoolSummaryResponse,
	PromotePlanResponse,
	PromoteRequest,
	PromoteResponse,
	PublishManifestRequest,
	PublishRequest,
	PublishResponse,
	PutPolicyRequest,
	SecretListResponse,
	SetConfigRequest,
	SetSecretRequest,
	StatusResponse,
	UpdateFunctionRequest,
	UploadArtifactResponse,
	VersionResponse,
} from "./generated-functions";

// The function API's types are generated from the platform's function
// document (`crates/fc-platform/resources/openapi/functions.openapi.json`,
// served at `GET /api/openapi-functions.json`: Java's, plus Rust's
// backward-compatible additions), so `vue-tsc` fails when the contract
// drifts. Every operation of that document is wrapped below except the four `/control/functions/*`
// routes: those are the function host's protocol, never called by the SPA.
export type {
	AliasResponse,
	CheckManifestResponse,
	ConfigResponse,
	DomainResponse,
	FunctionPageResponse,
	FunctionResponse,
	FunctionRouteResponse,
	Manifest,
	ManifestErrorResponse,
	PolicyResponse,
	PolicySignerRequest,
	PoolSummaryResponse,
	PromotePlanResponse,
	PromoteResponse,
	PublishManifestRequest,
	PublishResponse,
	SecretListResponse,
	StatusResponse,
	UploadArtifactResponse,
	VersionResponse,
};

export type FunctionStatus = FunctionResponse["status"];
export type FunctionRuntime = FunctionResponse["runtime"];

export interface FunctionListFilters {
	/** A function address pattern: `a.b.c`, `a.b.*`, or `a.*` (one application). */
	address?: string;
	/** An owning client id, or `platform`. */
	clientId?: string;
	status?: FunctionStatus;
	/** 0-based. */
	page?: number;
	size?: number;
}

export interface FunctionRouteFilters {
	hostname?: string;
	address?: string;
}

function fnPath(address: string, rest = ""): string {
	return `/functions/${encodeURIComponent(address)}${rest}`;
}

function versionQuery(version?: number): string {
	return version !== undefined ? `?version=${version}` : "";
}

export const functionsApi = {
	list(filters: FunctionListFilters = {}): Promise<FunctionPageResponse> {
		const params = new URLSearchParams();
		if (filters.address) params.set("address", filters.address);
		if (filters.clientId) params.set("clientId", filters.clientId);
		if (filters.status) params.set("status", filters.status);
		if (filters.page !== undefined) params.set("page", String(filters.page));
		if (filters.size !== undefined) params.set("size", String(filters.size));
		const query = params.toString();
		return apiFetch(`/functions${query ? `?${query}` : ""}`);
	},

	create(data: CreateFunctionRequest, options?: FetchOptions): Promise<FunctionResponse> {
		return apiFetch("/functions", {
			method: "POST",
			body: JSON.stringify(data),
			...options,
		});
	},

	get(address: string): Promise<FunctionResponse> {
		return apiFetch(fnPath(address));
	},

	/** 204 No Content: reload with `get` afterwards. */
	update(address: string, data: UpdateFunctionRequest): Promise<void> {
		return apiFetch(fnPath(address), {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	/** 204 No Content. Cascades to versions, aliases, routes, trigger objects and artifacts. */
	delete(address: string): Promise<void> {
		return apiFetch(fnPath(address), { method: "DELETE" });
	},

	status(address: string): Promise<StatusResponse> {
		return apiFetch(fnPath(address, "/status"));
	},

	pools(): Promise<PoolSummaryResponse[]> {
		return apiFetch("/function-pools");
	},

	publishVersion(address: string, data: PublishRequest, options?: FetchOptions): Promise<PublishResponse> {
		return apiFetch(fnPath(address, "/versions"), {
			method: "POST",
			body: JSON.stringify(data),
			...options,
		});
	},

	/**
	 * Validates a manifest and, when valid, returns the promote plan. Writes
	 * nothing. Same permission and reach as `publishVersion`.
	 */
	checkManifest(address: string, data: CheckManifestRequest): Promise<CheckManifestResponse> {
		return apiFetch(fnPath(address, "/manifest/check"), {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	/** The list shape leaves `manifest` out; `getVersion` includes it. */
	listVersions(address: string): Promise<VersionResponse[]> {
		return apiFetch(fnPath(address, "/versions"));
	},

	getVersion(address: string, version: number): Promise<VersionResponse> {
		return apiFetch(fnPath(address, `/versions/${version}`));
	},

	retireVersion(address: string, version: number): Promise<VersionResponse> {
		return apiFetch(fnPath(address, `/versions/${version}/retire`), {
			method: "POST",
		});
	},

	promoteAlias(address: string, alias: string, data: PromoteRequest): Promise<PromoteResponse> {
		return apiFetch(fnPath(address, `/aliases/${encodeURIComponent(alias)}`), {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	/** Points `alias` (default `live`) at `version`. `live` applies the manifest; any other alias is HTTP-only. */
	promote(address: string, version: number, alias = "live"): Promise<PromoteResponse> {
		return functionsApi.promoteAlias(address, alias, { version });
	},

	listAliases(address: string): Promise<AliasResponse[]> {
		return apiFetch(fnPath(address, "/aliases"));
	},

	/** 204 No Content: reload with `listAliases` afterwards. `live` is refused (`ALIAS_PROTECTED`). */
	deleteAlias(address: string, alias: string): Promise<void> {
		return apiFetch(fnPath(address, `/aliases/${encodeURIComponent(alias)}`), {
			method: "DELETE",
		});
	},

	/**
	 * Raw-bytes upload (`application/octet-stream`), keyed by the sha256 of
	 * the body. Returns the `artifactRef` a publish must use.
	 */
	uploadArtifact(address: string, digest: string, bytes: BodyInit, options?: FetchOptions): Promise<UploadArtifactResponse> {
		return apiFetch(fnPath(address, `/artifacts/${encodeURIComponent(digest)}`), {
			method: "PUT",
			body: bytes,
			headers: { "Content-Type": "application/octet-stream" },
			...options,
		});
	},

	getConfig(address: string, version?: number): Promise<ConfigResponse> {
		return apiFetch(fnPath(address, `/config${versionQuery(version)}`));
	},

	/** A full replacement of the config map; returns the new state. */
	setConfig(address: string, data: SetConfigRequest, version?: number): Promise<ConfigResponse> {
		return apiFetch(fnPath(address, `/config${versionQuery(version)}`), {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	/** Keys and metadata only, never a value. 503 `ENCRYPTION_UNCONFIGURED` without an app key. */
	listSecrets(address: string, version?: number, options?: FetchOptions): Promise<SecretListResponse> {
		return apiFetch(fnPath(address, `/secrets${versionQuery(version)}`), options);
	},

	/** 204 No Content: the value is never returned. */
	setSecret(address: string, key: string, data: SetSecretRequest, options?: FetchOptions): Promise<void> {
		return apiFetch(fnPath(address, `/secrets/${encodeURIComponent(key)}`), {
			method: "PUT",
			body: JSON.stringify(data),
			...options,
		});
	},

	/** 204 No Content. */
	deleteSecret(address: string, key: string, options?: FetchOptions): Promise<void> {
		return apiFetch(fnPath(address, `/secrets/${encodeURIComponent(key)}`), {
			method: "DELETE",
			...options,
		});
	},

	/** Every stored policy row (anchor only). */
	listPolicies(): Promise<PolicyListResponse> {
		return apiFetch("/function-policies");
	},

	/** An owner's effective policy; `stored: false` is the platform default. */
	getPolicy(owner: string): Promise<PolicyResponse> {
		return apiFetch(`/function-policies/${encodeURIComponent(owner)}`);
	},

	putPolicy(owner: string, data: PutPolicyRequest): Promise<PolicyResponse> {
		return apiFetch(`/function-policies/${encodeURIComponent(owner)}`, {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	claimDomain(data: ClaimRequest, options?: FetchOptions): Promise<DomainResponse> {
		return apiFetch("/function-domains", {
			method: "POST",
			body: JSON.stringify(data),
			...options,
		});
	},

	/** `clientId` is required on the wire: `platform` for platform-owned claims. */
	listDomains(clientId: string): Promise<DomainResponse[]> {
		const params = new URLSearchParams({ clientId });
		return apiFetch(`/function-domains?${params.toString()}`);
	},

	getDomain(hostname: string): Promise<DomainResponse> {
		return apiFetch(`/function-domains/${encodeURIComponent(hostname)}`);
	},

	/** 204 No Content. 409 `DOMAIN_IN_USE` while a live manifest routes to it. */
	releaseDomain(hostname: string, options?: FetchOptions): Promise<void> {
		return apiFetch(`/function-domains/${encodeURIComponent(hostname)}`, {
			method: "DELETE",
			...options,
		});
	},

	/** One of `hostname` or `address` is required (`address` wins). */
	listRoutes(filters: FunctionRouteFilters = {}): Promise<FunctionRouteResponse[]> {
		const params = new URLSearchParams();
		if (filters.hostname) params.set("hostname", filters.hostname);
		if (filters.address) params.set("address", filters.address);
		const query = params.toString();
		return apiFetch(`/function-routes${query ? `?${query}` : ""}`);
	},
};
