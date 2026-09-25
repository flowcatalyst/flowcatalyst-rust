<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
	connectionsApi,
	type Connection,
	type ConnectionStatus,
} from "@/api/connections";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";

const router = useRouter();
const route = useRoute();
const connections = ref<Connection[]>([]);
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
	{ label: "Paused", value: "PAUSED" },
];

onMounted(async () => {
	await loadConnections();
});

async function loadConnections() {
	loading.value = true;
	error.value = null;
	try {
		const response = await connectionsApi.list();
		connections.value = response.connections;
	} catch (e) {
		error.value =
			e instanceof Error ? e.message : "Failed to load connections";
	} finally {
		loading.value = false;
	}
}

function openDetail(id: string, edit = false) {
	void router.push({
		path: `/connections/${id}`,
		query: edit ? { ...route.query, edit: "true" } : route.query,
	});
}

function openCreate() {
	void router.push({ path: "/connections/new", query: route.query });
}

function getStatusSeverity(status: ConnectionStatus) {
	switch (status) {
		case "ACTIVE":
			return "success";
		case "PAUSED":
			return "warn";
		default:
			return "secondary";
	}
}

function formatDate(dateString: string) {
	return new Date(dateString).toLocaleDateString();
}

function getScopeLabel(conn: Connection) {
	if (conn.clientIdentifier) {
		return conn.clientIdentifier;
	}
	return "Anchor-level";
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Connections</h1>
        <p class="page-subtitle">Manage webhook connections for event delivery</p>
      </div>
      <Button label="Create Connection" icon="pi pi-plus" @click="openCreate" />
    </header>

    <Message v-if="error" severity="error" class="error-message">{{ error }}</Message>

    <div class="fc-card">
      <DataTable
        :value="connections"
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
            search-placeholder="Search connections..."
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
        <template #empty>No connections found</template>

        <Column field="code" header="Code" sortable>
          <template #body="{ data }">
            <code class="conn-code">{{ data.code }}</code>
          </template>
        </Column>
        <Column field="name" header="Name" sortable />
        <Column header="Scope" sortable>
          <template #body="{ data }">
            <span class="client-scope">{{ getScopeLabel(data) }}</span>
          </template>
        </Column>
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
              icon="pi pi-pencil"
              text
              rounded
              v-tooltip="'Edit'"
              @click.stop="openDetail(data.id, true)"
            />
          </template>
        </Column>
      </DataTable>
    </div>

    <!-- Drawer outlet: detail/create child routes render over this list -->
    <RouterView v-slot="{ Component }">
      <component :is="Component" @changed="loadConnections" />
    </RouterView>
  </div>
</template>

<style scoped>
.error-message {
  margin-bottom: 16px;
}

.conn-code {
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
