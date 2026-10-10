/**
 * Process lifecycle rules shared by the list, the drawer and the editor.
 * The server enforces them too (UpdateProcess: PROCESS_ARCHIVED;
 * DeleteProcess: PROCESS_NOT_ARCHIVED).
 */

/** An archived process is read-only. */
export function canEditProcess(
	p: { status: string } | null | undefined,
): boolean {
	return !!p && p.status !== "ARCHIVED";
}
