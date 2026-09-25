import { apiFetch } from "./client";
import type {
	ListOutputBody,
	RequestDto,
	StatusChangeResponse,
} from "./generated";

// Response types alias the generated contract (api/openapi.lock.json) so
// `vue-tsc` fails on backend drift. The list body is an anonymous struct in
// the backend handler, so the generator named it the unhelpfully generic
// `ListOutputBody` (`{ requests: RequestDto[] }`) — aliased under
// module-scoped names here.
export type ResetApprovalRequest = RequestDto;
export type ResetApprovalListResponse = ListOutputBody;

// Lost-device password-reset approval queue (Phase 8). Client-administrators
// review and approve/deny resets for users in their client(s).
export const resetApprovalsApi = {
	list(): Promise<ResetApprovalListResponse> {
		return apiFetch(`/reset-approvals`);
	},
	approve(id: string): Promise<StatusChangeResponse> {
		return apiFetch(`/reset-approvals/${id}/approve`, { method: "POST" });
	},
	deny(id: string): Promise<StatusChangeResponse> {
		return apiFetch(`/reset-approvals/${id}/deny`, { method: "POST" });
	},
};
