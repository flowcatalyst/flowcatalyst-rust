import { computed, type Ref } from "vue";
import {
	onBeforeRouteLeave,
	onBeforeRouteUpdate,
	useRoute,
	useRouter,
	type LocationQuery,
} from "vue-router";
import { useConfirm } from "primevue/useconfirm";

/**
 * Shared discard-changes confirmation, used both by EntityDrawer (UI-initiated
 * closes) and the route-leave guard below (browser Back / direct navigation).
 */
export function confirmDiscardChanges(
	confirm: ReturnType<typeof useConfirm>,
): Promise<boolean> {
	return new Promise((resolve) => {
		confirm.require({
			message: "You have unsaved changes. Discard them?",
			header: "Discard Changes",
			icon: "pi pi-exclamation-triangle",
			rejectProps: {
				label: "Keep Editing",
				severity: "secondary",
				outlined: true,
			},
			acceptProps: { label: "Discard", severity: "danger" },
			accept: () => resolve(true),
			reject: () => resolve(false),
		});
	});
}

interface DrawerRouteOptions {
	/** The list route this drawer sits over, e.g. "/subscriptions" */
	listPath: string;
	/** Route param carrying the entity key; default "id" (roles use "roleName") */
	paramKey?: string;
	/** When set and true, leaving the route prompts a discard confirmation */
	dirty?: Ref<boolean>;
	/** Drawer-only query params dropped when navigating back to the list */
	stripQuery?: string[];
}

/**
 * Navigation companion for drawer-hosted detail/create routes (nested children
 * of their list route). Owns the back-to-list navigation and the route-leave
 * dirty guard.
 *
 * Closes initiated through the drawer UI confirm inside EntityDrawer and then
 * call goToList(), which pre-approves the navigation so the leave guard does
 * not prompt a second time. Browser Back (and any other navigation) is caught
 * only by the leave guard.
 */
export function useDrawerRoute(options: DrawerRouteOptions) {
	const route = useRoute();
	const router = useRouter();
	const confirm = useConfirm();
	const strip = options.stripQuery ?? ["edit", "from"];

	let approved = false;

	/** Reactive entity-key param — drawer instances are reused across rows */
	const id = computed(() => {
		const raw = route.params[options.paramKey ?? "id"];
		return typeof raw === "string" ? raw : undefined;
	});

	function listQuery(): LocationQuery {
		const query: LocationQuery = {};
		for (const [key, value] of Object.entries(route.query)) {
			if (!strip.includes(key)) query[key] = value;
		}
		return query;
	}

	/** Close the drawer: navigate to the list, keeping its filter query. */
	function goToList() {
		approved = true;
		void router.push({ path: options.listPath, query: listQuery() });
	}

	/**
	 * Create→detail handoff. `replace` so browser Back from the detail drawer
	 * returns to the list, not the spent create form.
	 */
	function replaceToDetail(newId: string) {
		approved = true;
		void router.replace({
			path: `${options.listPath}/${newId}`,
			query: listQuery(),
		});
	}

	onBeforeRouteLeave(async () => {
		if (approved) return true;
		if (!options.dirty?.value) return true;
		return confirmDiscardChanges(confirm);
	});

	// The drawer is non-modal, so the list stays clickable while it's open:
	// switching rows is a same-record param update (leave guard doesn't fire).
	// Prompt before discarding an open edit form on a row switch.
	onBeforeRouteUpdate(async (to, from) => {
		const key = options.paramKey ?? "id";
		if (to.params[key] === from.params[key]) return true;
		if (approved) return true;
		if (!options.dirty?.value) return true;
		return confirmDiscardChanges(confirm);
	});

	return { id, goToList, replaceToDetail };
}
