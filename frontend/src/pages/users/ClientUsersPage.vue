<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useAuthStore } from "@/stores/auth";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";
import { useClientOptions } from "@/composables/useClientOptions";
import { usersApi, type User } from "@/api/users";
import UserImportDialog from "@/components/UserImportDialog.vue";

// Client-administrator user management. Scoped by construction: every endpoint
// it calls is one the backend permits a non-anchor administrator to use against
// their own client's users — so it never surfaces an anchor-only control or
// hits a scope 403. Detail/create render in a right-side drawer over this list
// (ClientUserDetailDrawer / ClientUserCreateDrawer); role, application,
// password and 2FA management live there now. The platform user-management
// page (anchor scope) stays separate.

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();
const { ensureLoaded: ensureClients, getLabel: getClientLabel } =
	useClientOptions();

const users = ref<User[]>([]);
const loading = ref(false);
const totalRecords = ref(0);

const listState = useListState(
	{
		filters: {
			q: { type: "string", key: "q" },
			clientId: { type: "string", key: "clientId" },
			active: { type: "boolean", key: "active" },
		},
		pageSize: 100,
		sortField: "createdAt",
		sortOrder: "asc",
	},
	() => loadUsers(),
);
const { filters, page, pageSize, sortField, sortOrder, onPage } = listState;

// Lazy table: the popup inputs write the listState refs directly and
// loadUsers() serializes them into API params; the specs only feed the badge.
const { activeFilterCount, clearAll } = useTableFilters(listState, [
	{ field: "clientId", param: "clientId" },
	{ field: "active", param: "active" },
]);

const statusFilterOptions = [
	{ label: "Active", value: true },
	{ label: "Inactive", value: false },
];

// The clients this administrator can act in. With more than one, expose a
// client filter (and the import dialog's target picker); with one, everything
// is implicit.
const clientOptions = computed(() =>
	authStore.accessibleClients.map((id) => ({
		label: getClientLabel(id),
		value: id,
	})),
);
const isMultiClient = computed(() => authStore.accessibleClients.length > 1);
const defaultClientId = computed(
	() => authStore.accessibleClients[0] ?? authStore.user?.clientId ?? "",
);

onMounted(async () => {
	await Promise.all([ensureClients(), loadUsers()]);
});

async function loadUsers() {
	loading.value = true;
	try {
		const response = await usersApi.list({
			type: "USER",
			clientId: filters.clientId.value || undefined,
			active:
				filters.active.value !== null ? filters.active.value : undefined,
			q: filters.q.value || undefined,
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

function addUser() {
	void router.push({
		path: "/client-administration/users/new",
		query: route.query,
	});
}

function viewUser(user: User) {
	void router.push({
		path: `/client-administration/users/${user.id}`,
		query: route.query,
	});
}

function editUser(user: User) {
	void router.push({
		path: `/client-administration/users/${user.id}`,
		query: { ...route.query, edit: "true" },
	});
}

function clientName(id: string | null | undefined): string {
	return id ? getClientLabel(id) : "—";
}

function formatDate(dateStr: string | undefined | null) {
	return dateStr ? new Date(dateStr).toLocaleDateString() : "—";
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">User Management</h1>
        <p class="page-subtitle">Manage users for your client</p>
      </div>
      <div class="header-actions">
        <UserImportDialog
          :client-options="clientOptions"
          :default-client-id="defaultClientId"
          @imported="loadUsers"
        />
        <Button label="Add User" icon="pi pi-user-plus" @click="addUser" />
      </div>
    </header>

    <!-- Table -->
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
              <FcFormField v-if="isMultiClient" label="Client">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.clientId.value"
                    :options="clientOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All Clients"
                    showClear
                    appendTo="self"
                  />
                </template>
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

        <Column field="name" header="Name" style="width: 22%">
          <template #body="{ data }">
            <span class="user-name">{{ data.name }}</span>
          </template>
        </Column>

        <Column field="email" header="Email" style="width: 26%">
          <template #body="{ data }">
            <span class="user-email">{{ data.email || '—' }}</span>
          </template>
        </Column>

        <Column header="Client" style="width: 16%">
          <template #body="{ data }">
            <span class="client-name-text">{{ clientName(data.clientId) }}</span>
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

        <Column field="createdAt" header="Created" style="width: 8%">
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
