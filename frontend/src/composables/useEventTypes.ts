import { ref, computed, watch } from "vue";
import { useListState } from "./useListState";
import {
	eventTypesApi,
	type EventType,
	type EventTypeFilters,
	type EventTypeStatus,
} from "@/api/event-types";

export function useEventTypes() {
	const eventTypes = ref<EventType[]>([]);
	const initialLoading = ref(true);
	const loading = ref(false);
	const error = ref<string | null>(null);

	// Use useListState for URL-synced filters (no pagination/sort needed for this page).
	// `q` feeds the table's client-side global filter only — no reload watcher.
	const listState = useListState({
		filters: {
			q: { type: "string", key: "q" },
			applications: { type: "array", key: "app" },
			subdomains: { type: "array", key: "sub" },
			aggregates: { type: "array", key: "agg" },
			status: { type: "string", key: "status" },
		},
	});
	const { filters, hasActiveFilters, clearFilters: clearListFilters, withSuppressed } = listState;

	// Filter options
	const applicationOptions = ref<string[]>([]);
	const subdomainOptions = ref<string[]>([]);
	const aggregateOptions = ref<string[]>([]);

	const statusOptions = [
		{ label: "Current", value: "CURRENT" },
		{ label: "Archived", value: "ARCHIVED" },
	];

	// Expose filter refs directly for backwards compat with EventTypeListPage
	const selectedApplications = filters.applications;
	const selectedSubdomains = filters.subdomains;
	const selectedAggregates = filters.aggregates;
	const selectedStatus = computed({
		get: () => filters.status.value || null,
		set: (val: string | null) => { filters.status.value = val || ""; },
	});

	// ---- Data loading ----

	async function loadEventTypes() {
		loading.value = true;
		error.value = null;

		try {
			const apiFilters: EventTypeFilters = {};
			if (selectedApplications.value.length)
				apiFilters.applications = selectedApplications.value;
			if (selectedSubdomains.value.length)
				apiFilters.subdomains = selectedSubdomains.value;
			if (selectedAggregates.value.length)
				apiFilters.aggregates = selectedAggregates.value;
			if (filters.status.value)
				apiFilters.status = filters.status.value as EventTypeStatus;

			const response = await eventTypesApi.list(apiFilters);
			eventTypes.value = response.items;
		} catch (e) {
			error.value =
				e instanceof Error ? e.message : "Failed to load event types";
		} finally {
			loading.value = false;
		}
	}

	async function loadApplications() {
		const response = await eventTypesApi.getApplications();
		applicationOptions.value = response.options;

		// Prune selections that no longer exist
		const valid = new Set(response.options);
		const pruned = selectedApplications.value.filter((s) => valid.has(s));
		if (pruned.length !== selectedApplications.value.length) {
			withSuppressed(() => {
				selectedApplications.value = pruned;
			});
		}
	}

	async function loadSubdomains() {
		const apps = selectedApplications.value.length
			? selectedApplications.value
			: undefined;
		const response = await eventTypesApi.getSubdomains(apps);
		subdomainOptions.value = response.options;

		const pruned = selectedSubdomains.value.filter((s) =>
			response.options.includes(s),
		);
		if (pruned.length !== selectedSubdomains.value.length) {
			withSuppressed(() => {
				selectedSubdomains.value = pruned;
			});
		}
	}

	async function loadAggregates() {
		const apps = selectedApplications.value.length
			? selectedApplications.value
			: undefined;
		const subs = selectedSubdomains.value.length
			? selectedSubdomains.value
			: undefined;
		const response = await eventTypesApi.getAggregates(apps, subs);
		aggregateOptions.value = response.options;

		const pruned = selectedAggregates.value.filter((a) =>
			response.options.includes(a),
		);
		if (pruned.length !== selectedAggregates.value.length) {
			withSuppressed(() => {
				selectedAggregates.value = pruned;
			});
		}
	}

	function clearFilters() {
		clearListFilters();
	}

	// Watch for filter changes — cascade dependent options + reload data
	watch(selectedApplications, () => {
		loadSubdomains();
		loadAggregates();
		loadEventTypes();
	});

	watch(selectedSubdomains, () => {
		loadAggregates();
		loadEventTypes();
	});

	watch(selectedAggregates, () => {
		loadEventTypes();
	});

	watch(() => filters.status.value, () => {
		loadEventTypes();
	});

	async function initialize() {
		await Promise.all([loadApplications(), loadSubdomains(), loadAggregates()]);
		await loadEventTypes();
		initialLoading.value = false;
	}

	return {
		// State
		eventTypes,
		initialLoading,
		loading,
		error,

		// List state (URL-synced filters incl. `q`) — for FcTableToolbar/useTableFilters
		listState,

		// Filters
		selectedApplications,
		selectedSubdomains,
		selectedAggregates,
		selectedStatus,
		hasActiveFilters,

		// Options
		applicationOptions,
		subdomainOptions,
		aggregateOptions,
		statusOptions,

		// Actions
		loadEventTypes,
		clearFilters,
		initialize,
	};
}
