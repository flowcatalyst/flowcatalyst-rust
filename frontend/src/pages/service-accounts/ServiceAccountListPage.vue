<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useRouter } from "vue-router";
import {
	serviceAccountsApi,
	type ServiceAccount,
} from "@/api/service-accounts";
import { useListState } from "@/composables/useListState";
import { useReturnTo } from "@/composables/useReturnTo";
import { useClientOptions } from "@/composables/useClientOptions";
import ClientFilter from "@/components/ClientFilter.vue";

const router = useRouter();
const { navigateToDetail } = useReturnTo();
const { ensureLoaded: ensureClients, getLabel: getClientLabel } = useClientOptions();

const serviceAccounts = ref<ServiceAccount[]>([]);
const loading = ref(true);

const { filters, hasActiveFilters, clearFilters: clearListFilters } = useListState({
	filters: {
		q: { type: "string" as const, key: "q" },
		clientId: { type: "string" as const, key: "clientId" },
		active: { type: "string" as const, key: "active" },
	},
}, () => {
	loadServiceAccounts();
});

const statusOptions = [
	{ label: "Active", value: "active" },
	{ label: "Inactive", value: "inactive" },
];

const filteredServiceAccounts = computed(() => {
	let result = serviceAccounts.value;

	// Client-side search filter (name/code)
	if (filters.q.value) {
		const query = filters.q.value.toLowerCase();
		result = result.filter(
			(sa) =>
				sa.name?.toLowerCase().includes(query) ||
				sa.code?.toLowerCase().includes(query),
		);
	}

	return result;
});

onMounted(async () => {
	await Promise.all([loadServiceAccounts(), ensureClients()]);
});

async function loadServiceAccounts() {
	loading.value = true;
	try {
		const response = await serviceAccountsApi.list({
			clientId: filters.clientId.value || undefined,
			active:
				filters.active.value === "active"
					? true
					: filters.active.value === "inactive"
						? false
						: undefined,
		});
		serviceAccounts.value = response.serviceAccounts;
	} catch (error) {
		console.error("Failed to fetch service accounts:", error);
	} finally {
		loading.value = false;
	}
}

function clearFilters() {
	clearListFilters();
}

function addServiceAccount() {
	router.push("/identity/service-accounts/new");
}

function viewServiceAccount(sa: ServiceAccount) {
	navigateToDetail(`/identity/service-accounts/${sa.id}`);
}

function editServiceAccount(sa: ServiceAccount) {
	navigateToDetail(`/identity/service-accounts/${sa.id}`, { edit: "true" });
}

function getClientName(clientId: string): string {
	return getClientLabel(clientId);
}

function getClientNames(clientIds: string[]): string {
	// The tier follows the links: no links is ANCHOR, which reaches every client.
	const first = clientIds?.[0];
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

    <!-- Filters -->
    <div class="fc-card filter-card">
      <div class="filter-row">
        <div class="filter-group">
          <label>Search</label>
          <IconField>
            <InputIcon class="pi pi-search" />
            <InputText
              v-model="filters.q.value"
              placeholder="Search by name or code..."
              class="filter-input"
            />
          </IconField>
        </div>

        <div class="filter-group">
          <label>Client</label>
          <ClientFilter
            v-model="filters.clientId.value"
            :multiple="false"
            class="filter-input"
          />
        </div>

        <div class="filter-group">
          <label>Status</label>
          <Select
            v-model="filters.active.value"
            :options="statusOptions"
            optionLabel="label"
            optionValue="value"
            placeholder="All Statuses"
            :showClear="true"
            class="filter-input"
          />
        </div>

        <div class="filter-actions">
          <Button
            v-if="hasActiveFilters"
            label="Clear Filters"
            icon="pi pi-filter-slash"
            text
            severity="secondary"
            @click="clearFilters"
          />
        </div>
      </div>
    </div>

    <!-- Data Table -->
    <div class="fc-card table-card">
      <div v-if="loading" class="loading-container">
        <ProgressSpinner strokeWidth="3" />
      </div>

      <DataTable
        v-else
        :value="filteredServiceAccounts"
        :paginator="true"
        :rows="100"
        :rowsPerPageOptions="[50, 100, 250, 500]"
        :showCurrentPageReport="true"
        currentPageReportTemplate="Showing {first} to {last} of {totalRecords} service accounts"
        stripedRows
        size="small"
      >
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
                icon="pi pi-eye"
                text
                rounded
                severity="secondary"
                @click="viewServiceAccount(data)"
                v-tooltip.top="'View'"
              />
              <Button
                icon="pi pi-pencil"
                text
                rounded
                severity="secondary"
                @click="editServiceAccount(data)"
                v-tooltip.top="'Edit'"
              />
            </div>
          </template>
        </Column>

        <template #empty>
          <div class="empty-message">
            <i class="pi pi-server"></i>
            <span>No service accounts found</span>
            <Button v-if="hasActiveFilters" label="Clear filters" link @click="clearFilters" />
          </div>
        </template>
      </DataTable>
    </div>
  </div>
</template>

<style scoped>
.filter-card {
  margin-bottom: 24px;
}

.filter-row {
  display: flex;
  flex-wrap: wrap;
  gap: 16px;
  align-items: flex-end;
}

.filter-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 200px;
}

.filter-group label {
  font-size: 13px;
  font-weight: 500;
  color: #475569;
}

.filter-input {
  width: 100%;
}

.filter-actions {
  margin-left: auto;
}

.table-card {
  padding: 0;
  overflow: hidden;
}

.loading-container {
  display: flex;
  justify-content: center;
  align-items: center;
  padding: 60px;
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

@media (max-width: 1024px) {
  .filter-row {
    flex-direction: column;
    align-items: stretch;
  }

  .filter-group {
    min-width: 100%;
  }

  .filter-actions {
    margin-left: 0;
    margin-top: 8px;
  }
}
</style>
