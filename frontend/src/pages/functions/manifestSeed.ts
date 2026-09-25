// An in-memory hand-off of a starting manifest from the Versions tab's
// "Import manifest" to the manifest editor page. A file's content can't
// travel in a URL, and it needn't survive a reload: the editor has its own
// import control. Taken once, per function address.
import type { ManifestModel } from "./manifestModel";

let pending: { address: string; model: ManifestModel } | null = null;

export function setPendingManifestSeed(address: string, model: ManifestModel): void {
	pending = { address, model };
}

/** The pending seed for `address`, cleared on read; null when there is none. */
export function takePendingManifestSeed(address: string): ManifestModel | null {
	if (!pending || pending.address !== address) return null;
	const { model } = pending;
	pending = null;
	return model;
}
