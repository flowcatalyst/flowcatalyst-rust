<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
	functionsApi,
	type FunctionResponse,
	type FunctionStatus,
	type PoolSummaryResponse,
} from "@/api/functions";
import { applicationsApi, type Application } from "@/api/applications";
import { useAuthStore } from "@/stores/auth";
import { isUnscopedUser, userHasPermission } from "@/stores/permissions";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";
import { useReturnTo } from "@/composables/useReturnTo";
import { useClientOptions } from "@/composables/useClientOptions";
import { formatDate, functionStatusSeverity } from "./format";

const router = useRouter();
const route = useRoute();
const { navigateToDetail } = useReturnTo();
const authStore = useAuthStore();
const clientOptions = useClientOptions();

const canManage = computed(() =>
	userHasPermission(authStore.user, "platform:function:function:manage"),
);
// A client-scoped caller already sees only its own client's functions (the
// server's reach rule), so the client filter only means something without one.
const showClientFilter = computed(() => isUnscopedUser(authStore.user));

const functions = ref<FunctionResponse[]>([]);
const total = ref(0);
const loading = ref(false);
const applications = ref<Application[]>([]);

const pools = ref<PoolSummaryResponse[]>([]);
const poolsLoading = ref(true);

const listState = useListState(
	{
		filters: {
			applicationCode: { type: "string", key: "application" },
			clientId: { type: "string", key: "client" },
			status: { type: "string", key: "status" },
		},
		pageSize: 20,
	},
	() => load(),
);
const { filters, page, pageSize, onPage } = listState;
// Server-side filters (lazy table): the toolbar popup only needs the badge
// count and Clear All.
const { activeFilterCount, clearAll } = useTableFilters(listState, [
	{ field: "applicationCode", param: "applicationCode" },
	{ field: "clientId", param: "clientId" },
	{ field: "status", param: "status" },
]);

const applicationOptions = computed(() =>
	applications.value.map((a) => ({ label: a.name, value: a.code })),
);
const ownerFilterOptions = computed(() => [
	{ label: "Platform", value: "platform" },
	...clientOptions.options.value,
]);
const statusOptions = [
	{ label: "Active", value: "ACTIVE" },
	{ label: "Disabled", value: "DISABLED" },
];

async function load() {
	loading.value = true;
	try {
		// The list takes an address pattern, not an application filter:
		// `<code>.*` is every function of one application.
		const result = await functionsApi.list({
			address: filters.applicationCode.value
				? `${filters.applicationCode.value}.*`
				: undefined,
			clientId: showClientFilter.value
				? filters.clientId.value || undefined
				: undefined,
			status: (filters.status.value as FunctionStatus | "") || undefined,
			page: page.value,
			size: pageSize.value,
		});
		functions.value = result.data;
		total.value = result.total;
	} catch (err) {
		console.error("Failed to load functions", err);
	} finally {
		loading.value = false;
	}
}

async function loadFilterOptions() {
	try {
		applications.value = (await applicationsApi.list()).applications;
	} catch (err) {
		console.error("Failed to load applications", err);
	}
	if (showClientFilter.value) {
		try {
			await clientOptions.ensureLoaded();
		} catch (err) {
			console.error("Failed to load clients", err);
		}
	}
}

async function loadPools() {
	poolsLoading.value = true;
	try {
		pools.value = await functionsApi.pools();
	} catch (err) {
		console.error("Failed to load function pools", err);
	} finally {
		poolsLoading.value = false;
	}
}

onMounted(async () => {
	void loadPools();
	await loadFilterOptions();
	await load();
});

function ownerLabel(fn: FunctionResponse): string {
	if (!fn.clientId) return "Platform";
	return clientOptions.getLabel(fn.clientId);
}

function viewFunction(fn: FunctionResponse) {
	navigateToDetail(`/functions/${encodeURIComponent(fn.address)}`);
}

function onRowClick(event: { data: FunctionResponse }) {
	viewFunction(event.data);
}

function openCreate() {
	void router.push({ path: "/functions/new", query: route.query });
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Functions</h1>
        <p class="page-subtitle">Code the platform hosts and invokes</p>
      </div>
      <Button
        v-if="canManage"
        label="New Function"
        icon="pi pi-plus"
        @click="openCreate"
      />
    </header>

    <div class="fc-card">
      <DataTable
        :value="functions"
        :loading="loading"
        :total-records="total"
        :rows="pageSize"
        :first="page * pageSize"
        lazy
        paginator
        :rows-per-page-options="[10, 20, 50, 100]"
        data-key="id"
        row-hover
        stripedRows
        emptyMessage="No functions found"
        :rowClass="() => 'clickable-row'"
        @row-click="onRowClick"
        @page="onPage"
      >
        <template #header>
          <FcTableToolbar
            :show-search="false"
            show-refresh
            :active-filter-count="activeFilterCount"
            :has-active-filters="listState.hasActiveFilters.value"
            @clear-all="clearAll"
            @refresh="load"
          >
            <template #filters>
              <FcFormField label="Application">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.applicationCode.value"
                    :options="applicationOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All applications"
                    filter
                    showClear
                    appendTo="self"
                  />
                </template>
              </FcFormField>
              <FcFormField v-if="showClientFilter" label="Owner">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.clientId.value"
                    :options="ownerFilterOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All owners"
                    filter
                    showClear
                    appendTo="self"
                  />
                </template>
              </FcFormField>
              <FcFormField label="Status">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.status.value"
                    :options="statusOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All statuses"
                    showClear
                    appendTo="self"
                  />
                </template>
              </FcFormField>
            </template>
          </FcTableToolbar>
        </template>
        <Column header="Address">
          <template #body="{ data }">
            <span class="font-mono text-sm">{{ data.address }}</span>
          </template>
        </Column>
        <Column header="Owner">
          <template #body="{ data }">
            <span v-if="data.clientId">{{ ownerLabel(data) }}</span>
            <span v-else class="scope-platform">Platform</span>
          </template>
        </Column>
        <Column header="Runtime" field="runtime" style="width: 7rem" />
        <Column header="Live Version" style="width: 8rem">
          <template #body="{ data }">
            <span v-if="data.live" class="live-version">v{{ data.live.version }}</span>
            <span v-else class="text-muted">—</span>
          </template>
        </Column>
        <Column header="Status" style="width: 8rem">
          <template #body="{ data }">
            <Tag :value="data.status" :severity="functionStatusSeverity(data.status)" />
          </template>
        </Column>
        <Column header="Updated" style="width: 14rem">
          <template #body="{ data }">
            <span class="text-sm">{{ formatDate(data.updatedAt) }}</span>
          </template>
        </Column>
        <Column header="" style="width: 4rem">
          <template #body="{ data }">
            <Button
              icon="pi pi-arrow-right"
              severity="secondary"
              text
              rounded
              @click.stop="viewFunction(data)"
            />
          </template>
        </Column>
      </DataTable>
    </div>

    <!-- Read-only: GET /api/function-pools carries a pool name and a host count only. -->
    <div class="fc-card pools-card">
      <h2 class="section-title">Function Pools</h2>
      <ProgressSpinner v-if="poolsLoading" style="width: 24px; height: 24px" />
      <p v-else-if="pools.length === 0" class="text-muted text-sm">No pools with a live host</p>
      <ul v-else class="pools-list">
        <li v-for="pool in pools" :key="pool.pool" class="pools-item">
          <span class="pool-name">{{ pool.pool }}</span>
          <span class="text-muted">{{ pool.hosts }} host{{ pool.hosts === 1 ? "" : "s" }}</span>
        </li>
      </ul>
    </div>

    <!-- Drawer outlet: the create child route renders over this list -->
    <RouterView v-slot="{ Component }">
      <component :is="Component" @changed="load" />
    </RouterView>
  </div>
</template>

<style scoped>
.font-mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.text-sm {
  font-size: 0.875rem;
}

.text-muted {
  color: var(--text-color-secondary);
}

.scope-platform {
  color: var(--text-color-secondary);
  font-style: italic;
}

.live-version {
  font-weight: 600;
}

.pools-card {
  margin-top: 16px;
}

.section-title {
  margin: 0 0 12px;
  font-size: 1rem;
  font-weight: 600;
}

.pools-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
}

.pools-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: var(--surface-ground);
  border: 1px solid var(--surface-border);
  border-radius: 6px;
  font-size: 0.875rem;
}

.pool-name {
  font-weight: 600;
}
</style>
