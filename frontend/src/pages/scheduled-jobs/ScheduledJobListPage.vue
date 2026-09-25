<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";
import {
	scheduledJobsApi,
	type ScheduledJob,
	type ScheduledJobsFilterOptions,
} from "@/api/scheduled-jobs";

const router = useRouter();
const route = useRoute();

const jobs = ref<ScheduledJob[]>([]);
const total = ref(0);
const loading = ref(false);

const listState = useListState(
	{
		filters: {
			clientIds: { type: "array", key: "clients" },
			applicationIds: { type: "array", key: "applications" },
			statuses: { type: "array", key: "statuses" },
			search: { type: "string", key: "q" },
		},
		pageSize: 20,
		sortField: "createdAt",
		sortOrder: "desc",
	},
	() => load(),
);
const { filters, page, pageSize, onPage } = listState;

// Lazy table: the DataTable filter meta isn't bound — popup inputs write the
// listState refs directly and load() serializes them into API params.
const { activeFilterCount, clearAll } = useTableFilters(
	listState,
	[
		{ field: "clientId", param: "clientIds" },
		{ field: "application", param: "applicationIds" },
		{ field: "status", param: "statuses" },
	],
	{ globalParam: "search" },
);

const filterOptions = ref<ScheduledJobsFilterOptions>({
	clients: [],
	applications: [],
	statuses: [],
});

async function loadFilterOptions() {
	try {
		filterOptions.value = await scheduledJobsApi.filterOptions();
	} catch (err) {
		console.error("Failed to load filter options", err);
	}
}

async function load() {
	loading.value = true;
	try {
		const result = await scheduledJobsApi.list({
			clientIds: filters.clientIds.value.length ? filters.clientIds.value : undefined,
			applicationIds: filters.applicationIds.value.length
				? filters.applicationIds.value
				: undefined,
			statuses: filters.statuses.value.length ? filters.statuses.value : undefined,
			search: filters.search.value || undefined,
			page: page.value,
			size: pageSize.value,
		});
		jobs.value = result.data;
		total.value = result.total;
	} catch (err) {
		console.error("Failed to load scheduled jobs", err);
	} finally {
		loading.value = false;
	}
}

onMounted(async () => {
	await loadFilterOptions();
	await load();
});

function createJob() {
	void router.push({ path: "/scheduled-jobs/create", query: route.query });
}

function viewJob(job: ScheduledJob) {
	void router.push({ path: `/scheduled-jobs/${job.id}`, query: route.query });
}

function onRowClick(event: { data: ScheduledJob }) {
	viewJob(event.data);
}

function statusSeverity(status: string): "success" | "warn" | "secondary" | "info" {
	switch (status) {
		case "ACTIVE":
			return "success";
		case "PAUSED":
			return "warn";
		case "ARCHIVED":
			return "secondary";
		default:
			return "info";
	}
}

function formatCrons(crons: string[]): string {
	if (crons.length === 0) return "—";
	if (crons.length === 1) return crons[0] ?? "";
	return `${crons[0]} (+${crons.length - 1})`;
}

function formatDate(s?: string): string {
	if (!s) return "—";
	return new Date(s).toLocaleString();
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Scheduled Jobs</h1>
        <p class="page-subtitle">Cron-triggered webhook jobs</p>
      </div>
      <Button label="New Scheduled Job" icon="pi pi-plus" @click="createJob" />
    </header>

    <div class="fc-card table-card">
      <DataTable
        :value="jobs"
        :loading="loading"
        :total-records="total"
        :rows="pageSize"
        :first="page * pageSize"
        lazy
        paginator
        :rows-per-page-options="[10, 20, 50, 100]"
        data-key="id"
        row-hover
        :rowClass="() => 'clickable-row'"
        selection-mode="single"
        stripedRows
        @row-click="onRowClick"
        @page="onPage"
      >
        <template #header>
          <FcTableToolbar
            v-model:search="filters.search.value"
            search-placeholder="Code or name…"
            :active-filter-count="activeFilterCount"
            :has-active-filters="listState.hasActiveFilters.value"
            @clear-all="clearAll"
          >
            <template #filters>
              <FcFormField label="Client">
                <template #default="{ id: fieldId }">
                  <MultiSelect
                    :id="fieldId"
                    v-model="filters.clientIds.value"
                    :options="filterOptions.clients"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All clients"
                    appendTo="self"
                  />
                </template>
              </FcFormField>
              <FcFormField label="Application">
                <template #default="{ id: fieldId }">
                  <MultiSelect
                    :id="fieldId"
                    v-model="filters.applicationIds.value"
                    :options="filterOptions.applications"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All applications"
                    appendTo="self"
                  />
                </template>
              </FcFormField>
              <FcFormField label="Status">
                <template #default="{ id: fieldId }">
                  <MultiSelect
                    :id="fieldId"
                    v-model="filters.statuses.value"
                    :options="filterOptions.statuses"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All statuses"
                    appendTo="self"
                  />
                </template>
              </FcFormField>
            </template>
          </FcTableToolbar>
        </template>

        <Column header="Code" field="code" style="width: 20%">
          <template #body="{ data }">
            <span class="font-mono text-sm">{{ data.code }}</span>
            <div v-if="data.hasActiveInstance" class="active-flag">
              <i class="pi pi-spinner pi-spin" /> running
            </div>
          </template>
        </Column>
        <Column header="Name" field="name" style="width: 15%" />
        <Column header="Client" style="width: 12%">
          <template #body="{ data }">
            <span v-if="data.clientName">{{ data.clientName }}</span>
            <span v-else class="scope-platform">Platform</span>
          </template>
        </Column>
        <Column header="Application" style="width: 12%">
          <template #body="{ data }">
            <span v-if="data.applicationName">{{ data.applicationName }}</span>
            <span v-else class="scope-platform">—</span>
          </template>
        </Column>
        <Column header="Crons" style="width: 16%">
          <template #body="{ data }">
            <span class="font-mono text-sm">{{ formatCrons(data.crons) }}</span>
            <div class="text-muted text-xs">{{ data.timezone }}</div>
          </template>
        </Column>
        <Column header="Status" style="width: 8rem">
          <template #body="{ data }">
            <Tag :value="data.status" :severity="statusSeverity(data.status)" />
          </template>
        </Column>
        <Column header="Last Fired" style="width: 12%">
          <template #body="{ data }">
            <span class="text-sm">{{ formatDate(data.lastFiredAt) }}</span>
          </template>
        </Column>
        <template #empty>
          <div class="empty-message">
            <span>No scheduled jobs found</span>
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
      <component :is="Component" @changed="load" />
    </RouterView>
  </div>
</template>

<style scoped>
.table-card {
  padding: 0;
  overflow: hidden;
}

.font-mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.text-sm { font-size: 0.875rem; }
.text-xs { font-size: 0.75rem; }
.text-muted { color: var(--text-color-secondary); }

.active-flag {
  font-size: 0.75rem;
  color: var(--orange-500, #f97316);
  margin-top: 0.25rem;
  display: flex;
  align-items: center;
  gap: 0.25rem;
}

.scope-platform {
  color: var(--text-color-secondary);
  font-style: italic;
}

.empty-message {
  text-align: center;
  padding: 32px 24px;
  color: #64748b;
}

.empty-message span {
  display: block;
  margin-bottom: 8px;
}
</style>
