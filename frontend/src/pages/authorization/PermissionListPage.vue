<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { permissionsApi, type Permission } from "@/api/permissions";
import { rolesApi, type ApplicationOption } from "@/api/roles";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";
import { toast } from "@/utils/errorBus";


const permissions = ref<Permission[]>([]);
const loading = ref(true);

// Create-permission dialog. Anyone who can manage roles can define a
// permission (anchor-gated server-side). The four segments form the canonical
// "application:context:aggregate:action" code.
const applications = ref<ApplicationOption[]>([]);
const showCreateDialog = ref(false);
const creating = ref(false);
const createError = ref<string | null>(null);
const createForm = ref({
	application: "",
	context: "",
	aggregate: "",
	action: "",
	description: "",
});

// Each segment must be a lowercase token so the assembled code is well-formed.
const segmentPattern = /^[a-z0-9-]+$/;

// Coerce natural input ("Approve Invoice") into a valid segment on blur,
// rather than leaving Create silently disabled with no explanation.
function slugifySegment(value: string): string {
	return value
		.toLowerCase()
		.replace(/\s+/g, "-")
		.replace(/[^a-z0-9-]/g, "")
		.replace(/-+/g, "-")
		.replace(/^-|-$/g, "");
}
const newPermString = computed(() => {
	const { application, context, aggregate, action } = createForm.value;
	return `${application}:${context.trim()}:${aggregate.trim()}:${action.trim()}`;
});
const canCreate = computed(
	() =>
		!!createForm.value.application &&
		[
			createForm.value.context,
			createForm.value.aggregate,
			createForm.value.action,
		].every((s) => segmentPattern.test(s.trim())),
);
const permissionExists = computed(() =>
	permissions.value.some((p) => p.permission === newPermString.value),
);

// Fetch-all page: every constraint runs in the DataTable's client-side engine.
const listState = useListState({
	filters: {
		q:           { type: "string", key: "q" },
		application: { type: "string", key: "app" },
		context:     { type: "string", key: "ctx" },
		action:      { type: "string", key: "action" },
	},
});
const { filters } = listState;

const { filters: tableFilters, activeFilterCount, clearAll } = useTableFilters(
	listState,
	[
		{ field: "application", param: "application" },
		{ field: "context", param: "context" },
		{ field: "action", param: "action" },
	],
);

// Compute unique filter options
const applicationOptions = computed(() => {
	const unique = [...new Set(permissions.value.map((p) => p.application))];
	return unique.toSorted().map((s: string) => ({ label: s, value: s }));
});

const contextOptions = computed(() => {
	let filtered = permissions.value;
	if (filters.application.value) {
		filtered = filtered.filter(
			(p) => p.application === filters.application.value,
		);
	}
	const unique = [...new Set(filtered.map((p) => p.context))];
	return unique.toSorted().map((c: string) => ({ label: c, value: c }));
});

const actionOptions = computed(() => [
	{ label: "view", value: "view" },
	{ label: "create", value: "create" },
	{ label: "update", value: "update" },
	{ label: "delete", value: "delete" },
	{ label: "retry", value: "retry" },
]);

onMounted(async () => {
	await Promise.all([loadPermissions(), loadApplications()]);
});

async function loadPermissions() {
	loading.value = true;
	try {
		const response = await permissionsApi.list();
		permissions.value = response.items;
	} catch {
	} finally {
		loading.value = false;
	}
}

async function loadApplications() {
	try {
		const response = await rolesApi.getApplications();
		applications.value = response.options;
	} catch {
	}
}

function openCreateDialog() {
	createForm.value = {
		application: applications.value[0]?.code ?? "",
		context: "",
		aggregate: "",
		action: "",
		description: "",
	};
	createError.value = null;
	showCreateDialog.value = true;
}

async function createPermission() {
	if (!canCreate.value || permissionExists.value || creating.value) return;
	creating.value = true;
	createError.value = null;
	try {
		await permissionsApi.create({
			application: createForm.value.application,
			context: createForm.value.context.trim(),
			aggregate: createForm.value.aggregate.trim(),
			action: createForm.value.action.trim(),
			description: createForm.value.description.trim() || undefined,
		});
		toast.success("Created", `Permission ${newPermString.value} created`);
		showCreateDialog.value = false;
		await loadPermissions();
	} catch (e) {
		createError.value =
			e instanceof Error ? e.message : "Failed to create permission";
	} finally {
		creating.value = false;
	}
}

function getActionSeverity(action: string) {
	switch (action) {
		case "view":
			return "info";
		case "create":
			return "success";
		case "update":
			return "warn";
		case "delete":
			return "danger";
		default:
			return "secondary";
	}
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Permissions</h1>
        <p class="page-subtitle">View all available permissions in the system</p>
      </div>
      <div class="header-right">
        <Button label="Create Permission" icon="pi pi-plus" @click="openCreateDialog" />
      </div>
    </header>

    <!-- Data Table -->
    <div class="fc-card table-card">
      <DataTable
        :value="permissions"
        :loading="loading"
        :filters="tableFilters"
        :globalFilterFields="['permission', 'description']"
        :paginator="true"
        :rows="100"
        :rowsPerPageOptions="[50, 100, 250, 500]"
        :showCurrentPageReport="true"
        currentPageReportTemplate="Showing {first} to {last} of {totalRecords} permissions"
        size="small"
      >
        <template #header>
          <FcTableToolbar
            v-model:search="filters.q.value"
            search-placeholder="Search permissions..."
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
                    :options="applicationOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All Applications"
                    showClear
                    appendTo="self"
                  />
                </template>
              </FcFormField>
              <!-- Context options cascade off the selected application -->
              <FcFormField label="Context">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.context.value"
                    :options="contextOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All Contexts"
                    showClear
                    appendTo="self"
                  />
                </template>
              </FcFormField>
              <FcFormField label="Action">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.action.value"
                    :options="actionOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All Actions"
                    showClear
                    appendTo="self"
                  />
                </template>
              </FcFormField>
            </template>
          </FcTableToolbar>
        </template>
        <Column header="Permission" style="width: 35%">
          <template #body="{ data }">
            <span class="permission-string">{{ data.permission }}</span>
          </template>
        </Column>

        <Column field="application" header="Application" style="width: 12%">
          <template #body="{ data }">
            <Tag :value="data.application" severity="secondary" />
          </template>
        </Column>

        <Column field="context" header="Context" style="width: 12%">
          <template #body="{ data }">
            <span>{{ data.context }}</span>
          </template>
        </Column>

        <Column field="aggregate" header="Aggregate" style="width: 12%">
          <template #body="{ data }">
            <span>{{ data.aggregate }}</span>
          </template>
        </Column>

        <Column field="action" header="Action" style="width: 10%">
          <template #body="{ data }">
            <Tag :value="data.action" :severity="getActionSeverity(data.action)" />
          </template>
        </Column>

        <Column field="description" header="Description" style="width: 19%">
          <template #body="{ data }">
            <span class="description-text" v-tooltip.top="data.description">
              {{ data.description || '—' }}
            </span>
          </template>
        </Column>

        <template #empty>
          <div class="empty-message">
            <i class="pi pi-lock"></i>
            <span>No permissions found</span>
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

    <!-- Create Permission Dialog -->
    <Dialog
      v-model:visible="showCreateDialog"
      header="Create Permission"
      :modal="true"
      :style="{ width: '560px' }"
      :closable="!creating"
    >
      <form class="dialog-form" @submit.prevent="createPermission">
        <div class="form-field">
          <label>Application <span class="required">*</span></label>
          <Select
            v-model="createForm.application"
            :options="applications"
            optionLabel="name"
            optionValue="code"
            placeholder="Select application"
            class="full-width"
          />
        </div>

        <div class="segments-row">
          <div class="form-field">
            <label>Context <span class="required">*</span></label>
            <InputText
              v-model="createForm.context"
              placeholder="e.g. billing"
              class="seg-input"
              @blur="createForm.context = slugifySegment(createForm.context)"
            />
          </div>
          <div class="form-field">
            <label>Aggregate <span class="required">*</span></label>
            <InputText
              v-model="createForm.aggregate"
              placeholder="e.g. invoice"
              class="seg-input"
              @blur="createForm.aggregate = slugifySegment(createForm.aggregate)"
            />
          </div>
          <div class="form-field">
            <label>Action <span class="required">*</span></label>
            <InputText
              v-model="createForm.action"
              placeholder="e.g. approve"
              class="seg-input"
              @blur="createForm.action = slugifySegment(createForm.action)"
            />
          </div>
        </div>

        <div class="form-field">
          <label>Description</label>
          <InputText v-model="createForm.description" placeholder="What this permission grants" class="full-width" />
        </div>

        <div class="preview-row">
          <span class="preview-label">Permission code</span>
          <code class="preview-code">{{ newPermString }}</code>
        </div>

        <small v-if="permissionExists" class="hint warn">
          This permission already exists.
        </small>
        <small v-else class="hint">
          Each segment is auto-formatted to lowercase letters, numbers and hyphens.
        </small>

        <Message v-if="createError" severity="error" class="error-message">
          {{ createError }}
        </Message>
      </form>

      <template #footer>
        <Button
          label="Cancel"
          icon="pi pi-times"
          severity="secondary"
          outlined
          :disabled="creating"
          @click="showCreateDialog = false"
        />
        <Button
          label="Create Permission"
          icon="pi pi-check"
          :loading="creating"
          :disabled="!canCreate || permissionExists"
          @click="createPermission"
        />
      </template>
    </Dialog>
  </div>
</template>

<style scoped>
.header-right {
  display: flex;
  align-items: center;
  gap: 8px;
}

.dialog-form {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.form-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.form-field label {
  font-size: 13px;
  font-weight: 500;
  color: #475569;
}

.required {
  color: #ef4444;
}

.segments-row {
  display: flex;
  gap: 12px;
}

.segments-row .form-field {
  flex: 1;
}

.seg-input {
  width: 100%;
  font-family: monospace;
}

.full-width {
  width: 100%;
}

.preview-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 6px;
}

.preview-label {
  font-size: 12px;
  font-weight: 600;
  color: #64748b;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.preview-code {
  font-family: monospace;
  font-size: 13px;
  color: #1e293b;
}

.hint {
  font-size: 12px;
  color: #64748b;
}

.hint.warn {
  color: #b45309;
}

.error-message {
  margin: 0;
}

.table-card {
  padding: 0;
  overflow: hidden;
}

.permission-string {
  font-family: monospace;
  font-size: 13px;
  color: #475569;
}

.description-text {
  color: #64748b;
  font-size: 13px;
  display: block;
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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
