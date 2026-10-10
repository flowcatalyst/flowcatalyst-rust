<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";
import { clientsApi, type Client } from "@/api/clients";

const PAGE_SIZE = 100;

const router = useRouter();
const route = useRoute();
const clients = ref<Client[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);

const listState = useListState(
	{
		filters: {
			q: { type: "string", key: "q" },
		},
		pageSize: PAGE_SIZE,
	},
	() => loadClients(),
);
const { filters } = listState;

// GET /api/clients returns every client (it has no paging parameters), so the
// list loads them all once and the table pages and searches them in the
// browser: the search covers every client, not one window.
const { filters: tableFilters, activeFilterCount, clearAll } = useTableFilters(
	listState,
	[],
);

onMounted(async () => {
	await loadClients();
});

async function loadClients() {
	loading.value = true;
	error.value = null;
	try {
		const response = await clientsApi.list();
		clients.value = response.clients;
	} catch (e) {
		error.value = e instanceof Error ? e.message : "Failed to load clients";
	} finally {
		loading.value = false;
	}
}

function openDetail(id: string, edit = false) {
	void router.push({
		path: `/clients/${id}`,
		query: edit ? { ...route.query, edit: "true" } : route.query,
	});
}

function openCreate() {
	void router.push({ path: "/clients/new", query: route.query });
}

function getStatusSeverity(status: string) {
	switch (status) {
		case "ACTIVE":
			return "success";
		case "SUSPENDED":
			return "warn";
		case "INACTIVE":
			return "secondary";
		default:
			return "secondary";
	}
}

function formatDate(dateString: string) {
	return new Date(dateString).toLocaleDateString();
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Clients</h1>
        <p class="page-subtitle">Manage customer clients and their configurations</p>
      </div>
      <Button label="Create Client" icon="pi pi-plus" @click="openCreate" />
    </header>

    <Message v-if="error" severity="error" class="error-message">{{ error }}</Message>

    <div class="fc-card">
      <DataTable
        :value="clients"
        :loading="loading"
        :filters="tableFilters"
        :globalFilterFields="['identifier', 'name']"
        paginator
        :rows="PAGE_SIZE"
        :rowsPerPageOptions="[50, 100, 250, 500]"
        stripedRows
        rowHover
        :rowClass="() => 'clickable-row'"
        @row-click="(e) => openDetail(e.data.id)"
      >
        <template #header>
          <FcTableToolbar
            v-model:search="filters.q.value"
            search-placeholder="Search clients..."
            :active-filter-count="activeFilterCount"
            :has-active-filters="listState.hasActiveFilters.value"
            @clear-all="clearAll"
          />
        </template>
        <template #empty>No clients found</template>

        <Column field="identifier" header="Identifier" sortable>
          <template #body="{ data }">
            <code class="client-code">{{ data.identifier }}</code>
          </template>
        </Column>
        <Column field="name" header="Name" sortable />
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
      <component :is="Component" @changed="loadClients" />
    </RouterView>
  </div>
</template>

<style scoped>
.error-message {
  margin-bottom: 16px;
}

.client-code {
  background: #f1f5f9;
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 13px;
}

</style>
