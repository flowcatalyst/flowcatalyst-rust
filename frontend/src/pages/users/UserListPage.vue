<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";
import { useClientOptions } from "@/composables/useClientOptions";
import { usersApi, type User } from "@/api/users";
import { rolesApi, type Role } from "@/api/roles";
import ClientFilter from "@/components/ClientFilter.vue";
import UserImportDialog from "@/components/UserImportDialog.vue";

const router = useRouter();
const route = useRoute();
const {
	ensureLoaded: ensureClients,
	getLabel: getClientLabel,
	options: clientOptions,
} = useClientOptions();

const users = ref<User[]>([]);
const availableRoles = ref<Role[]>([]);
const loading = ref(false);
const totalRecords = ref(0);

const listState = useListState(
	{
		filters: {
			q: { type: "string", key: "q" },
			clientId: { type: "string", key: "clientId" },
			active: { type: "boolean", key: "active" },
			roles: { type: "array", key: "roles" },
		},
		pageSize: 100,
		sortField: "createdAt",
		sortOrder: "asc",
	},
	() => loadUsers(),
);
const { filters, page, pageSize, sortField, sortOrder, onPage, onSort } =
	listState;

// Lazy table: the DataTable filter meta isn't bound — popup inputs write the
// listState refs directly and loadUsers() serializes them into API params.
const { activeFilterCount, clearAll } = useTableFilters(listState, [
	{ field: "clientId", param: "clientId" },
	{ field: "active", param: "active" },
	{ field: "roles", param: "roles" },
]);

const statusFilterOptions = [
	{ label: "Active", value: true },
	{ label: "Inactive", value: false },
];

const roleOptions = computed(() =>
	availableRoles.value.map((r) => ({ label: r.displayName, value: r.name })),
);

onMounted(async () => {
	await Promise.all([loadUsers(), ensureClients(), loadRoles()]);
});

async function loadUsers() {
	loading.value = true;
	try {
		const response = await usersApi.list({
			type: "USER",
			clientId: filters.clientId.value || undefined,
			active: filters.active.value !== null ? filters.active.value : undefined,
			q: filters.q.value || undefined,
			roles: filters.roles.value.length > 0 ? filters.roles.value : undefined,
			page: page.value,
			pageSize: pageSize.value,
			sortField: sortField.value,
			sortOrder: sortOrder.value,
		});
		users.value = response.principals;
		totalRecords.value = response.total;
	} catch (error) {
		console.error("Failed to fetch users:", error);
	} finally {
		loading.value = false;
	}
}

async function loadRoles() {
	try {
		const response = await rolesApi.list();
		availableRoles.value = response.items;
	} catch (error) {
		console.error("Failed to fetch roles:", error);
	}
}

function addUser() {
	void router.push({ path: "/users/new", query: route.query });
}

function viewUser(user: User) {
	void router.push({ path: `/users/${user.id}`, query: route.query });
}

function editUser(user: User) {
	void router.push({
		path: `/users/${user.id}`,
		query: { ...route.query, edit: "true" },
	});
}

function getClientName(clientId: string | null | undefined): string {
	if (!clientId) return "No Client";
	return getClientLabel(clientId);
}

function getUserType(user: User): {
	label: string;
	severity: string;
	tooltip: string;
} {
	if (user.isAnchorUser) {
		return {
			label: "Anchor",
			severity: "warn",
			tooltip: "Has access to all clients via anchor domain",
		};
	}

	const grantedCount = user.grantedClientIds?.length || 0;

	if (grantedCount > 0 || (!user.clientId && grantedCount === 0)) {
		return {
			label: "Partner",
			severity: "info",
			tooltip: user.clientId
				? `Home: ${getClientName(user.clientId)}, +${grantedCount} granted`
				: `Access to ${grantedCount} client(s)`,
		};
	}

	return {
		label: "Client",
		severity: "secondary",
		tooltip: `Home client: ${getClientName(user.clientId)}`,
	};
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
        <h1 class="page-title">Users</h1>
        <p class="page-subtitle">Manage platform users and their access</p>
      </div>
      <div class="header-actions">
        <UserImportDialog :client-options="clientOptions" @imported="loadUsers" />
        <Button label="Add User" icon="pi pi-user-plus" @click="addUser" />
      </div>
    </header>

    <!-- Data Table -->
    <div class="fc-card table-card">
      <DataTable
        :value="users"
        :loading="loading"
        :paginator="true"
        :first="page * pageSize"
        :rows="pageSize"
        :totalRecords="totalRecords"
        :rowsPerPageOptions="[50, 100, 250, 500]"
        :lazy="true"
        :showCurrentPageReport="true"
        currentPageReportTemplate="Showing {first} to {last} of {totalRecords} users"
        stripedRows
        rowHover
        :rowClass="() => 'clickable-row'"
        size="small"
        @page="onPage"
        @sort="onSort"
        @row-click="(e) => viewUser(e.data)"
      >
        <template #header>
          <FcTableToolbar
            v-model:search="filters.q.value"
            search-placeholder="Search by name or email..."
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
              <FcFormField label="Roles">
                <template #default="{ id: fieldId }">
                  <MultiSelect
                    :id="fieldId"
                    v-model="filters.roles.value"
                    :options="roleOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All roles"
                    display="chip"
                    appendTo="self"
                  />
                </template>
              </FcFormField>
            </template>
          </FcTableToolbar>
        </template>

        <Column field="name" header="Name" sortable style="width: 20%">
          <template #body="{ data }">
            <span class="user-name">{{ data.name }}</span>
          </template>
        </Column>

        <Column field="email" header="Email" sortable style="width: 25%">
          <template #body="{ data }">
            <span class="user-email">{{ data.email || '—' }}</span>
          </template>
        </Column>

        <Column header="Type" style="width: 12%">
          <template #body="{ data }">
            <Tag
              v-tooltip.top="getUserType(data).tooltip"
              :value="getUserType(data).label"
              :severity="getUserType(data).severity"
              :icon="data.isAnchorUser ? 'pi pi-star' : undefined"
            />
          </template>
        </Column>

        <Column header="Client" style="width: 15%">
          <template #body="{ data }">
            <div class="client-cell">
              <span v-if="data.isAnchorUser" class="all-clients-text">All Clients</span>
              <template v-else-if="data.clientId">
                <span class="client-name-text">{{ getClientName(data.clientId) }}</span>
                <span v-if="data.grantedClientIds?.length > 0" class="additional-clients">
                  +{{ data.grantedClientIds.length }} more
                </span>
              </template>
              <template v-else-if="data.grantedClientIds?.length > 0">
                <span class="client-name-text">{{ getClientName(data.grantedClientIds[0]) }}</span>
                <span v-if="data.grantedClientIds.length > 1" class="additional-clients">
                  +{{ data.grantedClientIds.length - 1 }} more
                </span>
              </template>
              <template v-else>
                <span class="no-client-text">No Client</span>
              </template>
            </div>
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

        <Column header="Actions" style="width: 60px">
          <template #body="{ data }">
            <div class="action-buttons">
              <Button
                v-tooltip.top="'Edit'"
                icon="pi pi-pencil"
                text
                rounded
                severity="secondary"
                @click.stop="editUser(data)"
              />
            </div>
          </template>
        </Column>

        <template #empty>
          <div class="empty-message">
            <i class="pi pi-users"></i>
            <span>No users found</span>
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
      <component :is="Component" @changed="loadUsers" />
    </RouterView>
  </div>
</template>

<style scoped>
.header-actions {
  display: flex;
  gap: 8px;
  align-items: center;
}

.table-card {
  padding: 0;
  overflow: hidden;
}

.user-name {
  font-weight: 500;
  color: #1e293b;
}

.user-email {
  color: #64748b;
  font-size: 13px;
}

.client-cell {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.client-name-text {
  font-size: 13px;
  color: #1e293b;
}

.all-clients-text {
  font-size: 13px;
  color: #f59e0b;
  font-weight: 500;
}

.no-client-text {
  font-size: 13px;
  color: #94a3b8;
  font-style: italic;
}

.additional-clients {
  font-size: 11px;
  color: #64748b;
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
