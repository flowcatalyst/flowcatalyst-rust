import { computed, type ComputedRef, type Ref } from "vue";
import { FilterMatchMode } from "@primevue/core/api";
import type { DataTableFilterMeta } from "primevue/datatable";

/** One filter bridged from a useListState ref into the DataTable filter engine. */
export interface ColumnFilterSpec {
	/** Row field the constraint applies to (dot paths supported by PrimeVue) */
	field: string;
	/** Name of the ref in listState.filters to read */
	param: string;
	/** Defaults to IN for array refs, EQUALS otherwise; pass CONTAINS for text */
	matchMode?: string;
}

interface ListStateLike {
	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	filters: Record<string, Ref<any>>;
	hasActiveFilters: ComputedRef<boolean>;
	clearFilters: () => void;
}

interface TableFilterOptions {
	/** listState param feeding the global quick-search; default "q" */
	globalParam?: string;
}

function isEmptyFilterValue(value: unknown): boolean {
	return (
		value === null ||
		value === undefined ||
		value === "" ||
		(Array.isArray(value) && value.length === 0)
	);
}

/** PrimeVue treats any non-null value as an active constraint — keep ""/[] out. */
function toFilterValue(value: unknown): unknown {
	return isEmptyFilterValue(value) ? null : value;
}

/**
 * Companion to the FcTableToolbar filter popup. Filter inputs live in the
 * toolbar's #filters slot and bind DIRECTLY to useListState refs (which own
 * URL sync, debounce, page reset, and the onChange refetch for lazy tables).
 *
 * This composable derives what the popup pattern still needs:
 * - `filters`: a DataTableFilterMeta computed from the listState refs. Bind it
 *   (`:filters` + `:globalFilterFields`) on CLIENT-SIDE tables so PrimeVue's
 *   built-in engine applies the constraints — no filter UI is mounted in the
 *   table itself. Lazy tables don't need it.
 * - `activeFilterCount`: badge for the toolbar's Filters button (excludes the
 *   always-visible global search).
 * - `clearAll`: resets inputs, URL, and badge in one click (and refetches on
 *   lazy tables via listState's onChange).
 */
export function useTableFilters(
	listState: ListStateLike,
	specs: ColumnFilterSpec[],
	options?: TableFilterOptions,
) {
	const globalParam = options?.globalParam ?? "q";
	const globalRef = listState.filters[globalParam];

	function matchModeFor(spec: ColumnFilterSpec): string {
		if (spec.matchMode) return spec.matchMode;
		const current = listState.filters[spec.param]?.value;
		return Array.isArray(current) ? FilterMatchMode.IN : FilterMatchMode.EQUALS;
	}

	const filters = computed<DataTableFilterMeta>(() => {
		const meta: DataTableFilterMeta = {};
		if (globalRef) {
			meta["global"] = {
				value: toFilterValue(globalRef.value),
				matchMode: FilterMatchMode.CONTAINS,
			};
		}
		for (const spec of specs) {
			meta[spec.field] = {
				value: toFilterValue(listState.filters[spec.param]?.value),
				matchMode: matchModeFor(spec),
			};
		}
		return meta;
	});

	/** Popup filters only — the global search is always visible in the toolbar. */
	const activeFilterCount = computed(
		() =>
			specs.filter(
				(spec) => !isEmptyFilterValue(listState.filters[spec.param]?.value),
			).length,
	);

	function clearAll() {
		listState.clearFilters();
	}

	return { filters, activeFilterCount, clearAll };
}
