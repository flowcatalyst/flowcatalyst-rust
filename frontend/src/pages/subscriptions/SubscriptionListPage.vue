<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { subscriptionsApi, type Subscription } from "@/api/subscriptions";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";

const router = useRouter();
const route = useRoute();
const subscriptions = ref<Subscription[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);

const listState = useListState({
	filters: {
		q: { type: "string" as const, key: "q" },
		status: { type: "string" as const, key: "status" },
		application: { type: "array" as const, key: "app" },
	},
});
const { filters } = listState;

const { filters: tableFilters, activeFilterCount, clearAll } = useTableFilters(
	listState,
	[
		{ field: "status", param: "status" },
		{ field: "applicationCode", param: "application" },
	],
);

const statusFilterOptions = [
	{ label: "Active", value: "ACTIVE" },
	{ label: "Paused", value: "PAUSED" },
];

const applicationOptions = computed(() => {
	const codes = new Set<string>();
	subscriptions.value.forEach((sub) => {
		if (sub.applicationCode) {
			codes.add(sub.applicationCode);
		}
	});
	return Array.from(codes)
		.toSorted()
		.map((code: string) => ({ label: code, value: code }));
});

onMounted(async () => {
	await loadSubscriptions();
});

async function loadSubscriptions() {
	loading.value = true;
	error.value = null;
	try {
		const response = await subscriptionsApi.list();
		subscriptions.value = response.subscriptions;
	} catch (e) {
		error.value =
			e instanceof Error ? e.message : "Failed to load subscriptions";
	} finally {
		loading.value = false;
	}
}

function openDetail(id: string, edit = false) {
	void router.push({
		path: `/subscriptions/${id}`,
		query: edit ? { ...route.query, edit: "true" } : route.query,
	});
}

function openCreate() {
	void router.push({ path: "/subscriptions/new", query: route.query });
}

// Wire status is plain string (spec carries no enum); default covers unknowns.
function getStatusSeverity(status: string) {
	switch (status) {
		case "ACTIVE":
			return "success";
		case "PAUSED":
			return "warn";
		default:
			return "secondary";
	}
}

function getModeLabel(mode: string) {
	switch (mode) {
		case "IMMEDIATE":
			return "Immediate";
		case "NEXT_ON_ERROR":
			return "Next on Error";
		case "BLOCK_ON_ERROR":
			return "Block on Error";
		default:
			return mode;
	}
}

function formatDate(dateString: string) {
	return new Date(dateString).toLocaleDateString();
}

function getScopeLabel(sub: Subscription) {
	if (sub.clientIdentifier) {
		return sub.clientIdentifier;
	}
	return "Anchor-level";
}

function getEventTypesLabel(sub: Subscription) {
	const count = sub.eventTypes?.length || 0;
	return `${count} event type${count !== 1 ? "s" : ""}`;
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Subscriptions</h1>
        <p class="page-subtitle">Manage event subscriptions and webhook routing</p>
      </div>
      <Button label="Create Subscription" icon="pi pi-plus" @click="openCreate" />
    </header>

    <Message v-if="error" severity="error" class="error-message">{{ error }}</Message>

    <div class="fc-card">
      <DataTable
        :value="subscriptions"
        :loading="loading"
        :filters="tableFilters"
        :globalFilterFields="['code', 'name', 'connectionId', 'applicationCode', 'clientIdentifier']"
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
            search-placeholder="Search subscriptions..."
            :active-filter-count="activeFilterCount"
            :has-active-filters="listState.hasActiveFilters.value"
            @clear-all="clearAll"
          >
            <template #filters>
              <FcFormField label="Application">
                <template #default="{ id: fieldId }">
                  <MultiSelect
                    :id="fieldId"
                    v-model="filters.application.value"
                    :options="applicationOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All applications"
                    display="chip"
                    appendTo="self"
                  />
                </template>
              </FcFormField>
              <FcFormField label="Status">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.status.value"
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
        <template #empty>No subscriptions found</template>

        <Column field="code" header="Code" sortable>
          <template #body="{ data }">
            <code class="sub-code">{{ data.code }}</code>
          </template>
        </Column>
        <Column field="applicationCode" header="Application" sortable>
          <template #body="{ data }">
            <code v-if="data.applicationCode" class="app-code">{{ data.applicationCode }}</code>
            <span v-else class="no-app">—</span>
          </template>
        </Column>
        <Column field="name" header="Name" sortable />
        <Column header="Scope" sortable>
          <template #body="{ data }">
            <span class="scope-label">{{ getScopeLabel(data) }}</span>
          </template>
        </Column>
        <Column header="Event Types">
          <template #body="{ data }">
            <span class="event-types-count">{{ getEventTypesLabel(data) }}</span>
          </template>
        </Column>
        <Column field="dispatchPoolCode" header="Pool" sortable>
          <template #body="{ data }">
            <code class="pool-code">{{ data.dispatchPoolCode }}</code>
          </template>
        </Column>
        <Column header="Mode">
          <template #body="{ data }">
            <span class="mode-label">{{ getModeLabel(data.mode) }}</span>
          </template>
        </Column>
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
      <component :is="Component" @changed="loadSubscriptions" />
    </RouterView>
  </div>
</template>

<style scoped>
.error-message {
  margin-bottom: 16px;
}

.sub-code {
  background: #f1f5f9;
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 13px;
}

.app-code {
  background: #fef3c7;
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 12px;
  color: #92400e;
}

.no-app {
  color: #94a3b8;
}

.pool-code {
  background: #e0f2fe;
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 12px;
  color: #0369a1;
}

.scope-label {
  font-size: 13px;
  color: #64748b;
}

.event-types-count {
  font-size: 13px;
}

.mode-label {
  font-size: 12px;
  color: #64748b;
}
</style>
