import { apiFetch } from "./client";
import type {
	AppDocsGroup as GenAppDocsGroup,
	DocListResponse as GenDocListResponse,
	DocResponse as GenDocResponse,
	DocSummary as GenDocSummary,
} from "./generated";

// Documentation (admin-gated): the platform's published pages (embedded in
// the server binary, so they always match the running build) plus each
// application's SDK-synced pages.
export type DocSummary = GenDocSummary;
export type AppDocsGroup = GenAppDocsGroup;
export type DocListResponse = GenDocListResponse;
export type DocResponse = GenDocResponse;

export const docsApi = {
	list(): Promise<DocListResponse> {
		return apiFetch("/docs");
	},

	getPlatform(slug: string): Promise<DocResponse> {
		return apiFetch(`/docs/platform/${encodeURIComponent(slug)}`);
	},

	getApplication(appCode: string, slug: string): Promise<DocResponse> {
		return apiFetch(
			`/docs/applications/${encodeURIComponent(appCode)}/${encodeURIComponent(slug)}`,
		);
	},
};
