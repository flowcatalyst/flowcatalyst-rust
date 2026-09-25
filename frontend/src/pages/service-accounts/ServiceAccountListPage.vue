<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { FilterMatchMode } from "@primevue/core/api";
import {
	serviceAccountsApi,
	type ServiceAccount,
} from "@/api/service-accounts";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";
import { useClientOptions } from "@/composables/useClientOptions";
import ClientFilter from "@/components/ClientFilter.vue";

const router = useRouter();
const route = useRoute();
const { ensureLoaded: ensureClients, getLabel: getClientLabel } = useClientOptions();

const serviceAccounts = ref<ServiceAccount[]>([]);
const loading = ref(true);

const listState = useListState(
	{
		filters: {
			q: { type: "string", key: "q" },
			clientId: { type: "string", key: "clientId" },
			active: { type: "boolean", key: "active" },
		},
	},
	() => loadServiceAccounts(),
);
const { filters } = listState;

// Hybrid table: clientId + active go to the API (listState onChange →
// loadServiceAccounts) while `q` filters client-side via the global filter.
// Rows carry `clientIds` (an array, no scalar clientId), so the client filter
// re-applies by membership via CONTAINS — the server-filtered rows already
// match, keeping it a no-op; `active` is an idempotent EQUALS.
const { filters: tableFilters, activeFilterCount, clearAll } = useTableFilters(
	listState,
	[
		{ field: "clientIds", param: "clientId", matchMode: FilterMatchMode.CONTAINS },
		{ field: "active", param: "active" },
	],
);

const statusFilterOptions = [
	{ label: "Active", value: true },
	{ label: "Inactive", value: false },
];

onMounted(async () => {
	await Promise.all([loadServiceAccounts(), ensureClients()]);
});

async function loadServiceAccounts() {
	loading.value = true;
	try {
		const response = await serviceAccountsApi.list({
			clientId: filters.clientId.value || undefined,
			active: filters.active.value !== null ? filters.active.value : undefined,
		});
		serviceAccounts.value = response.serviceAccounts;
	} catch (error) {
		console.error("Failed to fetch service accounts:", error);
	} finally {
		loading.value = false;
	}
}

function addServiceAccount() {
	void router.push({ path: "/identity/service-accounts/new", query: route.query });
}

function viewServiceAccount(sa: ServiceAccount) {
	void router.push({
		path: `/identity/service-accounts/${sa.id}`,
		query: route.query,
	});
}

function editServiceAccount(sa: ServiceAccount) {
	void router.push({
		path: `/identity/service-accounts/${sa.id}`,
		query: { ...route.query, edit: "true" },
	});
}

function getClientName(clientId: string): string {
	return getClientLabel(clientId);
}

function getClientNames(clientIds: string[]): string {
	if (!clientIds || clientIds.length === 0) return "All";
	const first = clientIds[0];
	if (first === undefined) return "All";
	if (clientIds.length === 1) return getClientName(first);
	if (clientIds.length <= 2)
		return clientIds.map((id) => getClientName(id)).join(", ");
	return `${getClientName(first)} +${clientIds.length - 1} more`;
}

function formatDate(dateStr: string | undefined | null) {
	if (!dateStr) return "—";
	return new Date(dateStr).toLocaleDateString();
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Service Accounts</h1>
        <p class="page-subtitle">Manage service accounts and webhook credentials</p>
      </div>
      <Button label="Add Service Account" icon="pi pi-plus" @click="addServiceAccount" />
    </header>

    <!-- Data Table -->
    <div class="fc-card table-card">
      <DataTable
        :value="serviceAccounts"
        :loading="loading"
        :filters="tableFilters"
        :globalFilterFields="['name', 'code']"
        :paginator="true"
        :rows="100"
        :rowsPerPageOptions="[50, 100, 250, 500]"
        :showCurrentPageReport="true"
        currentPageReportTemplate="Showing {first} to {last} of {totalRecords} service accounts"
        stripedRows
        size="small"
        rowHover
        :rowClass="() => 'clickable-row'"
        @row-click="(e) => viewServiceAccount(e.data)"
      >
        <template #header>
          <FcTableToolbar
            v-model:search="filters.q.value"
            search-placeholder="Search by name or code..."
            :active-filter-count="activeFilterCount"
            :has-active-filters="listState.hasActiveFilters.value"
            @clear-all="clearAll"
          >
            <template #filters>
              <FcFormField label="Client">
                <ClientFilter
                  v-model="filters.clientId.value"
                  :multiple="false"
                  appendTo="self"
                />
              </FcFormField>
              <FcFormField label="Status">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.active.value"
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

        <Column field="name" header="Name" sortable style="width: 20%">
          <template #body="{ data }">
            <span class="sa-name">{{ data.name }}</span>
          </template>
        </Column>

        <Column field="code" header="Code" sortable style="width: 15%">
          <template #body="{ data }">
            <code class="sa-code">{{ data.code }}</code>
          </template>
        </Column>

        <Column header="Auth Type" style="width: 12%">
          <template #body="{ data }">
            <Tag
              :value="data.authType || 'BEARER'"
              :severity="data.authType === 'BASIC' ? 'info' : 'secondary'"
            />
          </template>
        </Column>

        <Column header="Clients" style="width: 15%">
          <template #body="{ data }">
            <span class="client-name-text">{{ getClientNames(data.clientIds) }}</span>
          </template>
        </Column>

        <Column field="active" header="Status" style="width: 10%">
          <template #body="{ data }">
            <Tag
              :value="data.active ? 'Active' : 'Inactive'"
              :severity="data.active ? 'success' : 'danger'"
            />
          </template>
        </Column>

        <Column field="roles" header="Roles" style="width: 15%">
          <template #body="{ data }">
            <div class="roles-container">
              <Tag
                v-for="role in (data.roles || []).slice(0, 2)"
                :key="role"
                :value="role.split(':').pop()"
                severity="secondary"
                class="role-tag"
              />
              <span v-if="(data.roles || []).length > 2" class="more-roles">
                +{{ data.roles.length - 2 }} more
              </span>
            </div>
          </template>
        </Column>

        <Column field="createdAt" header="Created" sortable style="width: 10%">
          <template #body="{ data }">
            <span class="date-text">{{ formatDate(data.createdAt) }}</span>
          </template>
        </Column>

        <Column header="Actions" style="width: 5%">
          <template #body="{ data }">
            <div class="action-buttons">
              <Button
                v-tooltip.top="'Edit'"
                icon="pi pi-pencil"
                text
                rounded
                severity="secondary"
                @click.stop="editServiceAccount(data)"
              />
            </div>
          </template>
        </Column>

        <template #empty>
          <div class="empty-message">
            <i class="pi pi-server"></i>
            <span>No service accounts found</span>
            <Button
              v-if="listState.hasActiveFilters.value"
              label="Clear filters"
              link
              @click="clearAll"
            />
          </div>
        </template>
      </DataTable>
    </div>

    <!-- Drawer outlet: detail/create child routes render over this list -->
    <RouterView v-slot="{ Component }">
      <component :is="Component" @changed="loadServiceAccounts" />
    </RouterView>
  </div>
</template>

<style scoped>
.table-card {
  padding: 0;
  overflow: hidden;
}

.sa-name {
  font-weight: 500;
  color: #1e293b;
}

.sa-code {
  font-size: 12px;
  color: #64748b;
  background: #f1f5f9;
  padding: 2px 6px;
  border-radius: 4px;
}

.client-name-text {
  font-size: 13px;
  color: #1e293b;
}

.roles-container {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  align-items: center;
}

.role-tag {
  font-size: 11px;
}

.more-roles {
  font-size: 12px;
  color: #64748b;
}

.date-text {
  font-size: 13px;
  color: #64748b;
}

.action-buttons {
  display: flex;
  gap: 4px;
}

.empty-message {
  text-align: center;
  padding: 48px 24px;
  color: #64748b;
}

.empty-message i {
  font-size: 48px;
  display: block;
  margin-bottom: 16px;
  color: #cbd5e1;
}

.empty-message span {
  display: block;
  margin-bottom: 12px;
}

:deep(.p-datatable .p-datatable-thead > tr > th) {
  background: #f8fafc;
  color: #475569;
  font-weight: 600;
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}
</style>
