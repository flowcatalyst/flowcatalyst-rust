<script setup lang="ts">
import { ref, onMounted, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";
import ClientFilter from "@/components/ClientFilter.vue";
import {
	eventsApi,
	type EventRead,
	type EventsListParams,
} from "@/api/events";

interface FilterOption {
	value: string;
	label: string;
}

const router = useRouter();
const route = useRoute();

const sizeOptions = [50, 100, 200, 500, 1000];

// MANUAL loading: the cascading handlers below clear child refs under
// withSuppressed and call load() themselves. Do NOT pass an onChange
// callback here — its async watcher flush escapes withSuppressed and
// would fire one load per cleared child ref.
const listState = useListState({
	filters: {
		clients: { type: "array", key: "clients" },
		applications: { type: "array", key: "applications" },
		subdomains: { type: "array", key: "subdomains" },
		aggregates: { type: "array", key: "aggregates" },
		types: { type: "array", key: "types" },
		search: { type: "string", key: "q" },
	},
	pageSize: 200,
});
const { filters, pageSize, hasActiveFilters, clearFilters, syncToUrl, withSuppressed } =
	listState;

// Server-side filtering: the DataTable filter meta isn't bound — popup
// inputs write the listState refs directly and load() serializes them
// into API params. Only the badge count is derived here.
const { activeFilterCount } = useTableFilters(
	listState,
	[
		{ field: "clientId", param: "clients" },
		{ field: "application", param: "applications" },
		{ field: "subdomain", param: "subdomains" },
		{ field: "aggregate", param: "aggregates" },
		{ field: "type", param: "types" },
	],
	{ globalParam: "search" },
);

function buildParams(): EventsListParams {
	return {
		size: pageSize.value,
		clientIds: filters.clients.value.length ? filters.clients.value : undefined,
		applications: filters.applications.value.length ? filters.applications.value : undefined,
		subdomains: filters.subdomains.value.length ? filters.subdomains.value : undefined,
		aggregates: filters.aggregates.value.length ? filters.aggregates.value : undefined,
		types: filters.types.value.length ? filters.types.value : undefined,
		source: filters.search.value || undefined,
	};
}

const events = ref<EventRead[]>([]);
const loading = ref(false);

async function load() {
	loading.value = true;
	try {
		events.value = await eventsApi.list(buildParams());
	} catch (error) {
		console.error("Failed to load events:", error);
	} finally {
		loading.value = false;
	}
}

// Search reload: debounced, replacing the old Enter-to-search.
let searchTimer: ReturnType<typeof setTimeout> | undefined;
watch(filters.search, () => {
	clearTimeout(searchTimer);
	searchTimer = setTimeout(load, 400);
});

// Filter options (from server)
const applicationOptions = ref<FilterOption[]>([]);
const subdomainOptions = ref<FilterOption[]>([]);
const aggregateOptions = ref<FilterOption[]>([]);
const typeOptions = ref<FilterOption[]>([]);
const loadingOptions = ref(false);

onMounted(async () => {
	await loadFilterOptions();
	await load();
});

// Cascading clears: changing a parent wipes its dependent children. The
// child writes are wrapped in `withSuppressed` so useListState's per-ref
// watchers don't each spam syncToUrl; we sync once at the end.
function onClientsChange() {
	withSuppressed(() => {
		filters.applications.value = [];
		filters.subdomains.value = [];
		filters.aggregates.value = [];
		filters.types.value = [];
	});
	syncToUrl();
	load();
}

function onApplicationsChange() {
	withSuppressed(() => {
		filters.subdomains.value = [];
		filters.aggregates.value = [];
		filters.types.value = [];
	});
	syncToUrl();
	load();
}

function onSubdomainsChange() {
	withSuppressed(() => {
		filters.aggregates.value = [];
		filters.types.value = [];
	});
	syncToUrl();
	load();
}

function onAggregatesChange() {
	withSuppressed(() => {
		filters.types.value = [];
	});
	syncToUrl();
	load();
}

async function loadFilterOptions() {
	loadingOptions.value = true;
	try {
		const data = await eventsApi.filterOptions();
		applicationOptions.value = data.applications || [];
		subdomainOptions.value = data.subdomains || [];
		// `aggregates` is not surfaced by the shared filter-options endpoint.
		aggregateOptions.value = [];
		typeOptions.value = data.eventTypes || [];
	} catch (error) {
		console.error("Failed to load filter options:", error);
	} finally {
		loadingOptions.value = false;
	}
}

function clearAllFilters() {
	clearFilters();
	load();
}

function viewEventDetail(event: EventRead) {
	void router.push({ path: `/events/${event.id}`, query: route.query });
}

function formatDate(dateStr: string | undefined): string {
	if (!dateStr) return "-";
	return new Date(dateStr).toLocaleString();
}

function truncateId(id: string | undefined): string {
	if (!id) return "-";
	return id.length > 10 ? `${id.slice(0, 10)}...` : id;
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Events</h1>
        <p class="page-subtitle">Browse events from the event store</p>
      </div>
    </header>

    <div class="fc-card">
      <DataTable
        :value="events"
        :loading="loading"
        stripedRows
        emptyMessage="No events found"
        tableStyle="min-width: 60rem"
        rowHover
        :rowClass="() => 'clickable-row'"
        @row-click="(e) => viewEventDetail(e.data)"
      >
        <template #header>
          <FcTableToolbar
            v-model:search="filters.search.value"
            search-placeholder="Search by source..."
            :active-filter-count="activeFilterCount"
            :has-active-filters="hasActiveFilters"
            show-refresh
            @refresh="load"
            @clear-all="clearAllFilters"
          >
            <template #actions>
              <Select
                v-model="pageSize"
                :options="sizeOptions"
                class="size-select"
                @change="load"
                v-tooltip="'Result size — most recent N events'"
              />
            </template>
            <template #filters>
              <FcFormField label="Client">
                <ClientFilter
                  v-model="filters.clients.value"
                  appendTo="self"
                  @change="onClientsChange"
                />
              </FcFormField>
              <FcFormField label="Application">
                <template #default="{ id: fieldId }">
                  <MultiSelect
                    :id="fieldId"
                    v-model="filters.applications.value"
                    :options="applicationOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All Applications"
                    :maxSelectedLabels="2"
                    :loading="loadingOptions"
                    filter
                    appendTo="self"
                    @change="onApplicationsChange"
                  />
                </template>
              </FcFormField>
              <FcFormField label="Subdomain">
                <template #default="{ id: fieldId }">
                  <MultiSelect
                    :id="fieldId"
                    v-model="filters.subdomains.value"
                    :options="subdomainOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All Subdomains"
                    :maxSelectedLabels="2"
                    :loading="loadingOptions"
                    filter
                    appendTo="self"
                    @change="onSubdomainsChange"
                  />
                </template>
              </FcFormField>
              <FcFormField label="Aggregate">
                <template #default="{ id: fieldId }">
                  <MultiSelect
                    :id="fieldId"
                    v-model="filters.aggregates.value"
                    :options="aggregateOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All Aggregates"
                    :maxSelectedLabels="2"
                    :loading="loadingOptions"
                    filter
                    appendTo="self"
                    @change="onAggregatesChange"
                  />
                </template>
              </FcFormField>
              <FcFormField label="Event Type">
                <template #default="{ id: fieldId }">
                  <MultiSelect
                    :id="fieldId"
                    v-model="filters.types.value"
                    :options="typeOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All Types"
                    :maxSelectedLabels="1"
                    :loading="loadingOptions"
                    filter
                    appendTo="self"
                    @change="load"
                  />
                </template>
              </FcFormField>
            </template>
          </FcTableToolbar>
        </template>

        <Column field="id" header="Event ID" style="width: 10rem">
          <template #body="{ data }">
            <span class="font-mono text-sm">{{ truncateId(data.id) }}</span>
          </template>
        </Column>
        <Column field="type" header="Type">
          <template #body="{ data }">
            <Tag :value="data.type" severity="info" />
          </template>
        </Column>
        <Column field="source" header="Source" />
        <Column field="subject" header="Subject">
          <template #body="{ data }">
            <span class="text-sm truncate-cell">{{ data.subject || '-' }}</span>
          </template>
        </Column>
        <Column field="clientId" header="Client" style="width: 10rem">
          <template #body="{ data }">
            <span v-if="data.clientId" class="font-mono text-sm">{{
              truncateId(data.clientId)
            }}</span>
            <span v-else class="text-muted">-</span>
          </template>
        </Column>
        <Column field="time" header="Time" style="width: 12rem">
          <template #body="{ data }">
            <span class="text-sm">{{ formatDate(data.time) }}</span>
          </template>
        </Column>
      </DataTable>

      <!-- No pagination — events ingest at high rates and "page 2" is
           meaningless. Adjust size or narrow filters to see more. -->
      <div class="result-summary">
        Showing the {{ events.length }} most recent events
        <span v-if="events.length === pageSize"> (size limit reached — narrow filters or increase size)</span>
      </div>
    </div>

    <!-- Drawer outlet: the detail child route renders over this list -->
    <RouterView />
  </div>
</template>

<style scoped>
.size-select {
  width: 6rem;
}

.result-summary {
  text-align: center;
  font-size: 0.8125rem;
  color: var(--text-color-secondary);
  padding: 0.75rem 0 0.25rem;
}

.font-mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.text-sm {
  font-size: 0.875rem;
}

.text-muted {
  color: var(--text-color-secondary);
}

.truncate-cell {
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  display: inline-block;
}
</style>
