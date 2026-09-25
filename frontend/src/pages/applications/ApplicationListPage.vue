<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
	applicationsApi,
	type Application,
} from "@/api/applications";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";

const router = useRouter();
const route = useRoute();
const applications = ref<Application[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);

const listState = useListState({
	filters: {
		q: { type: "string" as const, key: "q" },
		type: { type: "string" as const, key: "type" },
		active: { type: "string" as const, key: "active" },
	},
});
const { filters } = listState;

const { filters: tableFilters, activeFilterCount, clearAll } = useTableFilters(
	listState,
	[
		{ field: "type", param: "type" },
		{ field: "status", param: "active" },
	],
);

const typeOptions = [
	{ label: "Application", value: "APPLICATION" },
	{ label: "Integration", value: "INTEGRATION" },
];

const activeOptions = [
	{ label: "Active", value: "ACTIVE" },
	{ label: "Inactive", value: "INACTIVE" },
];

// The wire row carries `active: boolean` while the filter value is the
// "ACTIVE"/"INACTIVE" string (same ?active= URL param as before), so derive a
// string `status` field for the EQUALS constraint to match against.
const rows = computed(() =>
	applications.value.map((app) => ({
		...app,
		status: app.active ? "ACTIVE" : "INACTIVE",
	})),
);

onMounted(async () => {
	await loadApplications();
});

async function loadApplications() {
	loading.value = true;
	error.value = null;
	try {
		const response = await applicationsApi.list();
		applications.value = response.applications;
	} catch (e) {
		error.value =
			e instanceof Error ? e.message : "Failed to load applications";
	} finally {
		loading.value = false;
	}
}

function openDetail(id: string, edit = false) {
	void router.push({
		path: `/applications/${id}`,
		query: edit ? { ...route.query, edit: "true" } : route.query,
	});
}

function openCreate() {
	void router.push({ path: "/applications/new", query: route.query });
}

function formatDate(dateString: string) {
	return new Date(dateString).toLocaleDateString();
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Applications</h1>
        <p class="page-subtitle">Manage applications in the platform ecosystem</p>
      </div>
      <Button label="Create Application" icon="pi pi-plus" @click="openCreate" />
    </header>

    <Message v-if="error" severity="error" class="error-message">{{ error }}</Message>

    <div class="fc-card">
      <DataTable
        :value="rows"
        :loading="loading"
        :filters="tableFilters"
        :globalFilterFields="['code', 'name', 'description']"
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
            search-placeholder="Search applications..."
            :active-filter-count="activeFilterCount"
            :has-active-filters="listState.hasActiveFilters.value"
            @clear-all="clearAll"
          >
            <template #filters>
              <FcFormField label="Type">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.type.value"
                    :options="typeOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All types"
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
                    :options="activeOptions"
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
        <template #empty>No applications found</template>

        <Column field="code" header="Code" sortable>
          <template #body="{ data }">
            <code class="app-code">{{ data.code }}</code>
          </template>
        </Column>
        <Column field="name" header="Name" sortable />
        <Column field="type" header="Type" sortable>
          <template #body="{ data }">
            <Tag
              :value="data.type === 'INTEGRATION' ? 'Integration' : 'Application'"
              :severity="data.type === 'INTEGRATION' ? 'info' : 'primary'"
            />
          </template>
        </Column>
        <Column field="description" header="Description">
          <template #body="{ data }">
            <span class="description-text">{{ data.description || '—' }}</span>
          </template>
        </Column>
        <Column field="active" header="Status" sortable>
          <template #body="{ data }">
            <Tag
              :value="data.active ? 'Active' : 'Inactive'"
              :severity="data.active ? 'success' : 'secondary'"
            />
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
      <component :is="Component" @changed="loadApplications" />
    </RouterView>
  </div>
</template>

<style scoped>
.error-message {
  margin-bottom: 16px;
}

.app-code {
  background: #f1f5f9;
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 13px;
}

.description-text {
  color: #64748b;
  font-size: 14px;
}
</style>
