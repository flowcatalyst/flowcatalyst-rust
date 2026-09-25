<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { processesApi } from "@/api/processes";
import type { Process } from "@/api/processes";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";

const router = useRouter();
const route = useRoute();

const processes = ref<Process[]>([]);
const loading = ref(true);

const listState = useListState({
	filters: {
		q: { type: "string" as const, key: "q" },
		application: { type: "string" as const, key: "app" },
		subdomain: { type: "string" as const, key: "subdomain" },
		status: { type: "string" as const, key: "status" },
	},
});
const { filters } = listState;

const { filters: tableFilters, activeFilterCount, clearAll } = useTableFilters(
	listState,
	[
		{ field: "application", param: "application" },
		{ field: "subdomain", param: "subdomain" },
		{ field: "status", param: "status" },
	],
);

const applicationOptions = computed(() => {
	const set = new Set(processes.value.map((p) => p.application));
	return Array.from(set).sort().map((v) => ({ label: v, value: v }));
});
const subdomainOptions = computed(() => {
	const set = new Set(
		processes.value
			.filter((p) => !filters.application.value || p.application === filters.application.value)
			.map((p) => p.subdomain),
	);
	return Array.from(set).sort().map((v) => ({ label: v, value: v }));
});
const statusOptions = [
	{ label: "Current", value: "CURRENT" },
	{ label: "Archived", value: "ARCHIVED" },
];

async function load() {
	loading.value = true;
	try {
		// Pull both CURRENT and ARCHIVED so the status filter can switch
		// without re-fetching. Volume is low — process docs are hand-curated.
		const res = await processesApi.list({});
		processes.value = res.items;
	} catch (e) {
		toast.error("Failed to load", (e as Error).message);
	} finally {
		loading.value = false;
	}
}

onMounted(() => load());

function viewProcess(p: Process) {
	void router.push({ path: `/processes/${p.id}`, query: route.query });
}

function editProcess(p: Process) {
	// Straight to the full-page Mermaid editor.
	void router.push(`/processes/${p.id}/edit`);
}

function openCreate() {
	// Full-page editor route — no drawer, so no filter-query carry needed.
	void router.push("/processes/create");
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Processes</h1>
        <p class="page-subtitle">
          Workflow documentation — how events, dispatch jobs, and reactive aggregates compose into a business process.
        </p>
      </div>
      <div class="header-actions">
        <Button
          label="Create Process"
          icon="pi pi-plus"
          @click="openCreate"
        />
      </div>
    </header>

    <div class="fc-card table-card">
      <DataTable
        :value="processes"
        :loading="loading"
        :filters="tableFilters"
        :globalFilterFields="['code', 'name']"
        :paginator="true"
        :rows="50"
        :rowsPerPageOptions="[25, 50, 100, 250]"
        :showCurrentPageReport="true"
        currentPageReportTemplate="Showing {first} to {last} of {totalRecords} processes"
        size="small"
        rowHover
        @row-click="(e) => viewProcess(e.data)"
        :rowClass="() => 'clickable-row'"
      >
        <template #header>
          <FcTableToolbar
            v-model:search="filters.q.value"
            search-placeholder="Search processes..."
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
                    placeholder="All applications"
                    showClear
                    appendTo="self"
                  />
                </template>
              </FcFormField>
              <FcFormField label="Subdomain">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.subdomain.value"
                    :options="subdomainOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All subdomains"
                    showClear
                    appendTo="self"
                  />
                </template>
              </FcFormField>
              <FcFormField label="Status">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.status.value"
                    :options="statusOptions"
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

        <Column header="Code" style="width: 30%">
          <template #body="{ data }">
            <div class="code-display">
              <span class="code-segment app">{{ data.application }}</span>
              <span class="code-separator">:</span>
              <span class="code-segment subdomain">{{ data.subdomain }}</span>
              <span class="code-separator">:</span>
              <span class="code-segment name">{{ data.processName }}</span>
            </div>
          </template>
        </Column>

        <Column field="name" header="Name" style="width: 22%">
          <template #body="{ data }">
            <span class="name-text">{{ data.name }}</span>
          </template>
        </Column>

        <Column field="description" header="Description" style="width: 28%">
          <template #body="{ data }">
            <span class="description-text" v-tooltip.top="data.description">
              {{ data.description || '—' }}
            </span>
          </template>
        </Column>

        <Column header="Tags" style="width: 12%">
          <template #body="{ data }">
            <div class="tag-list">
              <Tag
                v-for="t in data.tags"
                :key="t"
                :value="t"
                severity="secondary"
              />
              <span v-if="data.tags.length === 0" class="muted">—</span>
            </div>
          </template>
        </Column>

        <Column header="Status" style="width: 8%">
          <template #body="{ data }">
            <Tag
              :value="data.status"
              :severity="data.status === 'CURRENT' ? 'success' : 'secondary'"
            />
          </template>
        </Column>

        <Column header="Actions" style="width: 60px">
          <template #body="{ data }">
            <div class="action-buttons">
              <Button
                icon="pi pi-pencil"
                text
                rounded
                severity="secondary"
                v-tooltip.left="'Edit'"
                @click.stop="editProcess(data)"
              />
            </div>
          </template>
        </Column>

        <template #empty>
          <div class="empty-message">
            <i class="pi pi-inbox"></i>
            <span>No processes yet</span>
            <Button label="Create your first process" link @click="openCreate" />
          </div>
        </template>
      </DataTable>
    </div>

    <!-- Drawer outlet: the detail child route renders over this list -->
    <RouterView v-slot="{ Component }">
      <component :is="Component" @changed="load" />
    </RouterView>
  </div>
</template>

<style scoped>
.header-actions {
  display: flex;
  gap: 8px;
}

.action-buttons {
  display: flex;
  flex-wrap: nowrap;
  gap: 0;
}

.table-card {
  padding: 0;
  overflow: hidden;
}

.code-display {
  display: inline-flex;
  align-items: center;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 13px;
}

.code-segment.app { color: var(--p-primary-color); font-weight: 600; }
.code-segment.subdomain { color: var(--text-color); }
.code-segment.name { color: var(--text-color); font-weight: 500; }
.code-separator { color: var(--text-color-secondary); margin: 0 2px; }

.name-text { font-weight: 500; }
.description-text {
  color: var(--text-color-secondary);
  font-size: 13px;
  display: block;
  max-width: 320px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tag-list { display: flex; flex-wrap: wrap; gap: 4px; }
.muted { color: var(--text-color-secondary); font-size: 12px; }

.empty-message {
  text-align: center;
  padding: 48px 24px;
  color: var(--text-color-secondary);
}
.empty-message i {
  font-size: 48px;
  display: block;
  margin-bottom: 16px;
  color: var(--surface-border);
}
.empty-message span {
  display: block;
  margin-bottom: 12px;
}

:deep(.clickable-row) {
  cursor: pointer;
  transition: background-color 0.15s;
}
</style>
