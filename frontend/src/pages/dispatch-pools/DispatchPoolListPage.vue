<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { dispatchPoolsApi, type DispatchPool } from "@/api/dispatch-pools";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";

const router = useRouter();
const route = useRoute();
const pools = ref<DispatchPool[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);

const listState = useListState({
	filters: {
		q: { type: "string" as const, key: "q" },
		status: { type: "string" as const, key: "status" },
	},
});
const { filters } = listState;

const { filters: tableFilters, activeFilterCount, clearAll } = useTableFilters(
	listState,
	[{ field: "status", param: "status" }],
);

const statusFilterOptions = [
	{ label: "Active", value: "ACTIVE" },
	{ label: "Suspended", value: "SUSPENDED" },
	{ label: "Archived", value: "ARCHIVED" },
];

onMounted(async () => {
	await loadPools();
});

async function loadPools() {
	loading.value = true;
	error.value = null;
	try {
		const response = await dispatchPoolsApi.list();
		pools.value = response.pools;
	} catch (e) {
		error.value =
			e instanceof Error ? e.message : "Failed to load dispatch pools";
	} finally {
		loading.value = false;
	}
}

function openDetail(id: string, edit = false) {
	void router.push({
		path: `/dispatch-pools/${id}`,
		query: edit ? { ...route.query, edit: "true" } : route.query,
	});
}

function openCreate() {
	void router.push({ path: "/dispatch-pools/new", query: route.query });
}

// Wire status is plain string (spec carries no enum); default covers unknowns.
function getStatusSeverity(status: string) {
	switch (status) {
		case "ACTIVE":
			return "success";
		case "SUSPENDED":
			return "warn";
		case "ARCHIVED":
			return "secondary";
		default:
			return "secondary";
	}
}

function formatDate(dateString: string) {
	return new Date(dateString).toLocaleDateString();
}

function getScopeLabel(pool: DispatchPool) {
	if (pool.clientIdentifier) {
		return pool.clientIdentifier;
	}
	return "Anchor-level";
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Dispatch Pools</h1>
        <p class="page-subtitle">Manage rate limiting and concurrency for dispatch jobs</p>
      </div>
      <Button label="Create Pool" icon="pi pi-plus" @click="openCreate" />
    </header>

    <Message v-if="error" severity="error" class="error-message">{{ error }}</Message>

    <div class="fc-card">
      <DataTable
        :value="pools"
        :loading="loading"
        :filters="tableFilters"
        :globalFilterFields="['code', 'name', 'clientIdentifier']"
        paginator
        :rows="100"
        :rowsPerPageOptions="[50, 100, 250, 500]"
        stripedRows
        rowHover
        :rowClass="() => 'clickable-row'"
        @row-click="(e) => openDetail(e.data.id)"
      >
        <template #header>
          <FcTableToolbar
            v-model:search="filters.q.value"
            search-placeholder="Search pools..."
            :active-filter-count="activeFilterCount"
            :has-active-filters="listState.hasActiveFilters.value"
            @clear-all="clearAll"
          >
            <template #filters>
              <FcFormField label="Status">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.status.value"
                    :options="statusFilterOptions"
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
        <template #empty>No dispatch pools found</template>

        <Column field="code" header="Code" sortable>
          <template #body="{ data }">
            <code class="pool-code">{{ data.code }}</code>
          </template>
        </Column>
        <Column field="name" header="Name" sortable />
        <Column header="Client Scope" sortable>
          <template #body="{ data }">
            <span class="client-scope">{{ getScopeLabel(data) }}</span>
          </template>
        </Column>
        <Column field="rateLimit" header="Rate Limit" sortable>
          <template #body="{ data }">
            <span v-if="data.rateLimit != null">{{ data.rateLimit }}/min</span>
            <span v-else>—</span>
          </template>
        </Column>
        <Column field="concurrency" header="Concurrency" sortable />
        <Column field="status" header="Status" sortable>
          <template #body="{ data }">
            <Tag :value="data.status" :severity="getStatusSeverity(data.status)" />
          </template>
        </Column>
        <Column field="createdAt" header="Created" sortable>
          <template #body="{ data }">
            {{ formatDate(data.createdAt) }}
          </template>
        </Column>
        <Column header="Actions" style="width: 80px">
          <template #body="{ data }">
            <Button
              v-tooltip="'Edit'"
              icon="pi pi-pencil"
              text
              rounded
              @click.stop="openDetail(data.id, true)"
            />
          </template>
        </Column>
      </DataTable>
    </div>

    <!-- Drawer outlet: detail/create child routes render over this list -->
    <RouterView v-slot="{ Component }">
      <component :is="Component" @changed="loadPools" />
    </RouterView>
  </div>
</template>

<style scoped>
.error-message {
  margin-bottom: 16px;
}

.pool-code {
  background: #f1f5f9;
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 13px;
}

.client-scope {
  font-size: 13px;
  color: #475569;
}
</style>
