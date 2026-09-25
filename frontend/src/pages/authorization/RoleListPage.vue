<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useConfirm } from "primevue/useconfirm";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";
import {
	rolesApi,
	type Role,
	type RoleSource,
	type ApplicationOption,
} from "@/api/roles";

const router = useRouter();
const route = useRoute();
const confirm = useConfirm();

// Data
const roles = ref<Role[]>([]);
const applications = ref<ApplicationOption[]>([]);
const loading = ref(true);

// Hybrid list: application/source are server-side filters (onChange refetch);
// q stays client-side via the DataTable filter engine below.
const listState = useListState(
	{
		filters: {
			q: { type: "string", key: "q" },
			application: { type: "string", key: "app" },
			source: { type: "string", key: "source" },
		},
	},
	() => loadRoles(),
);
const { filters } = listState;

// application/source rows arrive pre-filtered from the server, so their meta
// constraints are no-ops here — the specs still feed the toolbar badge.
const { filters: tableFilters, activeFilterCount, clearAll } = useTableFilters(
	listState,
	[
		{ field: "applicationCode", param: "application" },
		{ field: "source", param: "source" },
	],
);

const sourceOptions = [
	{ label: "Code-defined", value: "CODE" },
	{ label: "Admin-created", value: "DATABASE" },
	{ label: "SDK-registered", value: "SDK" },
];

// Create dialog
const showCreateDialog = ref(false);
const createForm = ref({
	applicationCode: "",
	name: "",
	displayName: "",
	description: "",
});
const creating = ref(false);
const createError = ref<string | null>(null);

const isCreateFormValid = computed(() => {
	return createForm.value.applicationCode && createForm.value.name.trim();
});

// Initialize
onMounted(async () => {
	await Promise.all([loadRoles(), loadApplications()]);
});

async function loadRoles() {
	loading.value = true;
	try {
		const apiFilters: { application?: string; source?: RoleSource } = {};
		if (filters.application.value)
			apiFilters.application = filters.application.value;
		if (filters.source.value)
			apiFilters.source = filters.source.value as RoleSource;

		const response = await rolesApi.list(apiFilters);
		roles.value = response.items;
	} catch {
	} finally {
		loading.value = false;
	}
}

async function loadApplications() {
	try {
		const response = await rolesApi.getApplications();
		applications.value = response.options;
	} catch (e) {
		console.error("Failed to load applications:", e);
	}
}

function viewRole(role: Role) {
	// Role names contain ":" — encode so the name stays a single path segment.
	void router.push({
		path: `/authorization/roles/${encodeURIComponent(role.name)}`,
		query: route.query,
	});
}

function editRole(role: Role) {
	// Straight to the full-page permission-catalogue editor.
	void router.push(
		`/authorization/roles/${encodeURIComponent(role.name)}/edit`,
	);
}

function openCreateDialog() {
	createForm.value = {
		applicationCode:
			applications.value.length > 0 ? (applications.value[0]?.code ?? "") : "",
		name: "",
		displayName: "",
		description: "",
	};
	createError.value = null;
	showCreateDialog.value = true;
}

async function createRole() {
	if (!isCreateFormValid.value) return;

	creating.value = true;
	createError.value = null;

	try {
		await rolesApi.create({
			applicationCode: createForm.value.applicationCode,
			roleName: createForm.value.name,
			displayName: createForm.value.displayName || createForm.value.name,
			description: createForm.value.description || undefined,
		});

		toast.success("Success", "Role created successfully");
		showCreateDialog.value = false;
		loadRoles();
	} catch (e) {
		createError.value =
			e instanceof Error ? e.message : "Failed to create role";
	} finally {
		creating.value = false;
	}
}

function confirmDeleteRole(role: Role) {
	confirm.require({
		message: `Are you sure you want to delete the role "${role.displayName || role.name}"?`,
		header: "Delete Role",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Delete",
		acceptClass: "p-button-danger",
		accept: () => void deleteRole(role),
	});
}

async function deleteRole(role: Role) {
	try {
		await rolesApi.delete(role.name);
		toast.success("Success", "Role deleted successfully");
		loadRoles();
	} catch {
	}
}

function getSourceSeverity(source: RoleSource) {
	switch (source) {
		case "CODE":
			return "info";
		case "DATABASE":
			return "success";
		case "SDK":
			return "warn";
		default:
			return "secondary";
	}
}

function getSourceLabel(source: RoleSource) {
	switch (source) {
		case "CODE":
			return "Code";
		case "DATABASE":
			return "Admin";
		case "SDK":
			return "SDK";
		default:
			return source;
	}
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Roles</h1>
        <p class="page-subtitle">Manage roles and their permissions</p>
      </div>
      <Button label="Create Role" icon="pi pi-plus" @click="openCreateDialog" />
    </header>

    <!-- Data Table -->
    <div class="fc-card table-card">
      <DataTable
        :value="roles"
        :loading="loading"
        :filters="tableFilters"
        :globalFilterFields="['name', 'displayName', 'description']"
        :paginator="true"
        :rows="100"
        :rowsPerPageOptions="[50, 100, 250, 500]"
        :showCurrentPageReport="true"
        currentPageReportTemplate="Showing {first} to {last} of {totalRecords} roles"
        size="small"
        @row-click="(e) => viewRole(e.data)"
        :rowHover="true"
        :rowClass="() => 'clickable-row'"
      >
        <template #header>
          <FcTableToolbar
            v-model:search="filters.q.value"
            search-placeholder="Search roles..."
            :active-filter-count="activeFilterCount"
            :has-active-filters="listState.hasActiveFilters.value"
            @clear-all="clearAll"
          >
            <template #filters>
              <FcFormField label="Application">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.application.value"
                    :options="applications"
                    optionLabel="name"
                    optionValue="code"
                    placeholder="All applications"
                    showClear
                    filter
                    filterPlaceholder="Type to filter..."
                    autoFilterFocus
                    resetFilterOnHide
                    appendTo="self"
                  />
                </template>
              </FcFormField>
              <FcFormField label="Source">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.source.value"
                    :options="sourceOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All sources"
                    showClear
                    appendTo="self"
                  />
                </template>
              </FcFormField>
            </template>
          </FcTableToolbar>
        </template>

        <Column header="Role" style="width: 25%">
          <template #body="{ data }">
            <div class="role-info clickable">
              <span class="role-name">{{ data.displayName || data.shortName }}</span>
              <span class="role-code">{{ data.name }}</span>
            </div>
          </template>
        </Column>

        <Column field="description" header="Description" style="width: 30%">
          <template #body="{ data }">
            <span class="description-text" v-tooltip.top="data.description">
              {{ data.description || '—' }}
            </span>
          </template>
        </Column>

        <Column header="Permissions" style="width: 10%">
          <template #body="{ data }">
            <span class="permission-count">
              {{ data.permissions?.length || 0 }}
            </span>
          </template>
        </Column>

        <Column field="applicationCode" header="Application" style="width: 15%">
          <template #body="{ data }">
            <Tag :value="data.applicationCode" severity="secondary" />
          </template>
        </Column>

        <Column header="Source" style="width: 10%">
          <template #body="{ data }">
            <Tag :value="getSourceLabel(data.source)" :severity="getSourceSeverity(data.source)" />
          </template>
        </Column>

        <Column header="Actions" style="width: 7%">
          <template #body="{ data }">
            <div class="action-buttons" @click.stop>
              <Button
                v-if="data.source === 'DATABASE'"
                icon="pi pi-pencil"
                text
                rounded
                severity="secondary"
                v-tooltip.left="'Edit role'"
                @click="editRole(data)"
              />
              <Button
                v-if="data.source === 'DATABASE'"
                icon="pi pi-trash"
                text
                rounded
                severity="danger"
                v-tooltip.left="'Delete role'"
                @click="confirmDeleteRole(data)"
              />
            </div>
          </template>
        </Column>

        <template #empty>
          <div class="empty-message">
            <i class="pi pi-inbox"></i>
            <span>No roles found</span>
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

    <!-- Create Role Dialog -->
    <Dialog
      v-model:visible="showCreateDialog"
      header="Create Role"
      :modal="true"
      :closable="true"
      :style="{ width: '500px' }"
    >
      <form @submit.prevent="createRole">
        <div class="dialog-form">
          <div class="form-field">
            <label>Application <span class="required">*</span></label>
            <Select
              v-model="createForm.applicationCode"
              :options="applications"
              optionLabel="name"
              optionValue="code"
              placeholder="Select application"
              class="full-width"
            />
          </div>

          <div class="form-field">
            <label>Role Name <span class="required">*</span></label>
            <InputText
              v-model="createForm.name"
              placeholder="e.g., admin, viewer, manager"
              class="full-width"
            />
            <small class="field-hint">
              Will be prefixed with application code (e.g., "myapp:admin")
            </small>
          </div>

          <div class="form-field">
            <label>Display Name</label>
            <InputText
              v-model="createForm.displayName"
              placeholder="e.g., Administrator"
              class="full-width"
            />
          </div>

          <div class="form-field">
            <label>Description</label>
            <Textarea
              v-model="createForm.description"
              placeholder="What this role grants access to"
              :rows="3"
              class="full-width"
            />
          </div>

          <Message v-if="createError" severity="error" class="error-message">
            {{ createError }}
          </Message>
        </div>
      </form>

      <template #footer>
        <Button
          label="Cancel"
          icon="pi pi-times"
          severity="secondary"
          outlined
          @click="showCreateDialog = false"
        />
        <Button
          label="Create Role"
          icon="pi pi-check"
          :loading="creating"
          :disabled="!isCreateFormValid"
          @click="createRole"
        />
      </template>
    </Dialog>

    <!-- Drawer outlet: the role detail child route renders over this list -->
    <RouterView v-slot="{ Component }">
      <component :is="Component" @changed="loadRoles" />
    </RouterView>
  </div>
</template>

<style scoped>
.table-card {
  padding: 0;
  overflow: hidden;
}

.role-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.role-info.clickable {
  cursor: pointer;
}

.role-name {
  font-weight: 500;
  color: #1e293b;
}

.role-code {
  font-size: 12px;
  color: #64748b;
  font-family: monospace;
}

.description-text {
  color: #64748b;
  font-size: 13px;
  display: block;
  max-width: 300px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.permission-count {
  font-weight: 500;
  color: #475569;
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

/* Dialog Form Styles */
.dialog-form {
  padding: 8px 0;
}

.form-field {
  margin-bottom: 20px;
}

.form-field > label {
  display: block;
  font-weight: 500;
  margin-bottom: 6px;
}

.form-field .required {
  color: #ef4444;
}

.full-width {
  width: 100%;
}

.field-hint {
  display: block;
  font-size: 12px;
  color: #94a3b8;
  margin-top: 4px;
}

.error-message {
  margin-top: 16px;
}

:deep(.p-datatable .p-datatable-thead > tr > th) {
  background: #f8fafc;
  color: #475569;
  font-weight: 600;
  font-size: 13px;
  text-transform: uppercase;
  letter-spacing: 0.025em;
}

:deep(.p-datatable .p-datatable-tbody > tr) {
  cursor: pointer;
}

:deep(.p-datatable .p-datatable-tbody > tr:hover) {
  background: #f8fafc;
}
</style>
