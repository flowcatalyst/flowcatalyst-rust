/**
 * Event type lifecycle rules shared by the list and the detail drawer.
 *
 * canArchive and canDelete are also enforced by the server's use cases
 * (ArchiveEventType: SCHEMAS_NOT_DEPRECATED; DeleteEventType:
 * DELETE_NOT_ALLOWED), so the buttons and the API agree.
 */

import type { EventType } from "@/api/event-types";

type Rules = Pick<EventType, "status" | "specVersions">;

/** An archived event type is read-only: no edit, no new schema. */
export function canEditEventType(et: Rules | null | undefined): boolean {
	return !!et && et.status !== "ARCHIVED";
}

/** CURRENT, and every schema version (if any) DEPRECATED. */
export function canArchiveEventType(et: Rules | null | undefined): boolean {
	if (!et || et.status !== "CURRENT") return false;
	return et.specVersions.every((sv) => sv.status === "DEPRECATED");
}

/** ARCHIVED, or CURRENT with every schema version (if any) still FINALISING. */
export function canDeleteEventType(et: Rules | null | undefined): boolean {
	if (!et) return false;
	if (et.status === "ARCHIVED") return true;
	return (
		et.status === "CURRENT" &&
		et.specVersions.every((sv) => sv.status === "FINALISING")
	);
}
