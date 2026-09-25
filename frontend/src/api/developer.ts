// Stays hand-rolled: BFF endpoint — deliberately stripped from the OpenAPI
// spec (StripBFFPaths), so no generated types exist for it.
import { bffFetch } from "./client";

/**
 * Developer portal API client. Cookie-authenticated reads scoped to the
 * applications the current principal has access to via
 * `iam_principal_application_access` (anchor users see all).
 */

export interface DeveloperApplicationSummary {
	id: string;
	code: string;
	name: string;
	description: string | null;
	iconUrl: string | null;
	currentVersion: string | null;
	currentSpecId: string | null;
	currentSyncedAt: string | null;
}

export interface DeveloperApplicationsResponse {
	items: DeveloperApplicationSummary[];
}

export interface ChangeNotes {
	addedPaths: string[];
	removedPaths: string[];
	addedSchemas: string[];
	removedSchemas: string[];
	removedOperations: string[];
	hasBreaking: boolean;
}

export interface OpenApiSpecResponse {
	id: string;
	applicationId: string;
	version: string;
	status: "CURRENT" | "ARCHIVED";
	spec: Record<string, unknown>;
	changeNotesText: string | null;
	changeNotes: ChangeNotes | null;
	syncedAt: string;
}

export interface OpenApiVersionSummary {
	id: string;
	version: string;
	status: "CURRENT" | "ARCHIVED";
	changeNotesText: string | null;
	hasBreaking: boolean;
	syncedAt: string;
}

export interface OpenApiVersionsResponse {
	items: OpenApiVersionSummary[];
}

export interface DeveloperSpecVersionSummary {
	id: string;
	version: string;
	status: string;
	schema: string | null;
}

export interface DeveloperEventTypeSummary {
	id: string;
	code: string;
	name: string;
	description: string | null;
	status: string;
	application: string;
	subdomain: string;
	aggregate: string;
	eventName: string;
	specVersions: DeveloperSpecVersionSummary[];
}

export interface DeveloperEventTypesResponse {
	items: DeveloperEventTypeSummary[];
}

export interface SyncPlatformOpenApiResponse {
	applicationCode: string;
	specId: string;
	version: string;
	status: "CURRENT" | "UNCHANGED";
	archivedPriorVersion: string | null;
	hasBreaking: boolean;
	unchanged: boolean;
}

export const developerApi = {
	listApplications(): Promise<DeveloperApplicationsResponse> {
		return bffFetch("/developer/applications");
	},

	getApplication(id: string): Promise<DeveloperApplicationSummary> {
		return bffFetch(`/developer/applications/${id}`);
	},

	getCurrentOpenApi(id: string): Promise<OpenApiSpecResponse> {
		// A 404 here means "this application hasn't published its OpenAPI doc
		// yet" — the UI surfaces that as a friendly note, not an error toast.
		return bffFetch(`/developer/applications/${id}/openapi/current`, {
			suppressGlobalErrorToast: true,
		});
	},

	listOpenApiVersions(id: string): Promise<OpenApiVersionsResponse> {
		return bffFetch(`/developer/applications/${id}/openapi/versions`);
	},

	getOpenApiVersion(
		id: string,
		specId: string,
	): Promise<OpenApiSpecResponse> {
		return bffFetch(`/developer/applications/${id}/openapi/versions/${specId}`);
	},

	listEventTypes(id: string): Promise<DeveloperEventTypesResponse> {
		return bffFetch(`/developer/applications/${id}/event-types`);
	},

	syncPlatformOpenApi(): Promise<SyncPlatformOpenApiResponse> {
		return bffFetch("/developer/sync-platform-openapi", { method: "POST" });
	},
};
