<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useEventTypes } from "@/composables/useEventTypes";
import { useTableFilters } from "@/composables/useTableFilters";
import { eventTypesApi } from "@/api/event-types";
import type { EventType } from "@/api/event-types";
import { exportSchemasAsZip } from "@/utils/schema-export";

const router = useRouter();
const route = useRoute();
const syncing = ref(false);

const {
	eventTypes,
	initialLoading,
	loading,
	listState,
	selectedApplications,
	selectedSubdomains,
	selectedAggregates,
	selectedStatus,
	hasActiveFilters,
	applicationOptions,
	subdomainOptions,
	aggregateOptions,
	statusOptions,
	loadEventTypes,
	initialize,
} = useEventTypes();

// Facet constraints are also applied server-side by useEventTypes' watchers;
// mirroring them into the table filter meta keeps the toolbar badge accurate
// and gives `q` real client-side filtering via globalFilterFields.
const { filters: tableFilters, activeFilterCount, clearAll } = useTableFilters(
	listState,
	[
		{ field: "application", param: "applications" },
		{ field: "subdomain", param: "subdomains" },
		{ field: "aggregate", param: "aggregates" },
		{ field: "status", param: "status" },
	],
);

onMounted(() => initialize());

function viewEventType(eventType: EventType, edit = false) {
	void router.push({
		path: `/event-types/${eventType.id}`,
		query: edit ? { ...route.query, edit: "true" } : route.query,
	});
}

function openCreate() {
	void router.push({ path: "/event-types/create", query: route.query });
}

async function syncPlatformEvents() {
	syncing.value = true;
	try {
		const result = await eventTypesApi.syncPlatform();
		const parts: string[] = [];
		if (result.created > 0) parts.push(`${result.created} created`);
		if (result.updated > 0) parts.push(`${result.updated} updated`);
		if (result.deleted > 0) parts.push(`${result.deleted} deleted`);

		const schemaParts: string[] = [];
		if (result.schemas.created > 0) schemaParts.push(`${result.schemas.created} created`);
		if (result.schemas.updated > 0) schemaParts.push(`${result.schemas.updated} updated`);
		if (result.schemas.unchanged > 0) schemaParts.push(`${result.schemas.unchanged} unchanged`);
		const schemaTotal = result.schemas.created + result.schemas.updated + result.schemas.unchanged;
		const schemaDetail = schemaTotal > 0
			? `\nSchemas: ${schemaParts.join(", ")} (${schemaTotal} total)`
			: "";

		toast.success("Platform Events Synced", (parts.length > 0
				? `${parts.join(", ")} (${result.total} total)`
				: `${result.total} event types up to date`) + schemaDetail);
		await loadEventTypes();
	} catch {
	} finally {
		syncing.value = false;
	}
}

const hasSchemas = computed(() =>
	eventTypes.value.some((et) =>
		et.specVersions.some((sv) => sv.status === "CURRENT" && sv.schema),
	),
);

function exportSchemas() {
	const result = exportSchemasAsZip(eventTypes.value);

	if (result.exported === 0) {
		toast.warn("Nothing to Export", "No event types with a CURRENT schema in the current view");
		return;
	}

	toast.success("Export Complete", `Exported ${result.exported} schema${result.exported !== 1 ? "s" : ""} to event-schemas.zip`);

	if (result.errors.length > 0) {
		toast.warn(`${result.errors.length} Generation Error${result.errors.length !== 1 ? "s" : ""}`, result.errors.join("\n"));
	}
}

function getSchemaStatusSeverity(status: string) {
	switch (status) {
		case "CURRENT":
			return "success";
		case "FINALISING":
			return "info";
		case "DEPRECATED":
			return "warn";
		default:
			return "secondary";
	}
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Event Types</h1>
        <p class="page-subtitle">Manage event type definitions and schemas</p>
      </div>
      <div class="header-actions">
        <Button
          v-if="hasSchemas"
          label="Export Schemas"
          icon="pi pi-download"
          severity="secondary"
          outlined
          @click="exportSchemas"
        />
        <Button
          label="Sync Platform Events"
          icon="pi pi-sync"
          severity="secondary"
          :loading="syncing"
          @click="syncPlatformEvents"
        />
        <Button
          label="Create Event Type"
          icon="pi pi-plus"
          @click="openCreate"
        />
      </div>
    </header>

    <!-- Data Table -->
    <div class="fc-card table-card">
      <DataTable
        :value="eventTypes"
        :loading="initialLoading || loading"
        :filters="tableFilters"
        :globalFilterFields="['code', 'name', 'event', 'application', 'subdomain', 'aggregate']"
        :paginator="true"
        :rows="100"
        :rowsPerPageOptions="[50, 100, 250, 500]"
        :showCurrentPageReport="true"
        currentPageReportTemplate="Showing {first} to {last} of {totalRecords} event types"
        size="small"
        rowHover
        @row-click="(e) => viewEventType(e.data)"
        :rowClass="() => 'clickable-row'"
      >
        <template #header>
          <FcTableToolbar
            v-model:search="listState.filters.q.value"
            search-placeholder="Search event types..."
            :active-filter-count="activeFilterCount"
            :has-active-filters="hasActiveFilters"
            @clear-all="clearAll"
          >
            <template #filters>
              <FcFormField label="Applications">
                <template #default="{ id: fieldId }">
                  <MultiSelect
                    :id="fieldId"
                    v-model="selectedApplications"
                    :options="applicationOptions"
                    placeholder="All Applications"
                    :showClear="true"
                    :maxSelectedLabels="2"
                    selectedItemsLabel="{0} apps selected"
                    appendTo="self"
                  />
                </template>
              </FcFormField>
              <FcFormField label="Subdomains">
                <template #default="{ id: fieldId }">
                  <MultiSelect
                    :id="fieldId"
                    v-model="selectedSubdomains"
                    :options="subdomainOptions"
                    placeholder="All Subdomains"
                    :showClear="true"
                    :maxSelectedLabels="2"
                    selectedItemsLabel="{0} subdomains selected"
                    appendTo="self"
                  />
                </template>
              </FcFormField>
              <FcFormField label="Aggregates">
                <template #default="{ id: fieldId }">
                  <MultiSelect
                    :id="fieldId"
                    v-model="selectedAggregates"
                    :options="aggregateOptions"
                    placeholder="All Aggregates"
                    :showClear="true"
                    :maxSelectedLabels="2"
                    selectedItemsLabel="{0} aggregates selected"
                    appendTo="self"
                  />
                </template>
              </FcFormField>
              <FcFormField label="Status">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="selectedStatus"
                    :options="statusOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All Statuses"
                    :showClear="true"
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
              <span class="code-segment aggregate">{{ data.aggregate }}</span>
              <span class="code-separator">:</span>
              <span class="code-segment event">{{ data.event }}</span>
            </div>
          </template>
        </Column>

        <Column field="name" header="Name" style="width: 20%">
          <template #body="{ data }">
            <span class="name-text">{{ data.name }}</span>
          </template>
        </Column>

        <Column field="description" header="Description" style="width: 25%">
          <template #body="{ data }">
            <span class="description-text" v-tooltip.top="data.description">
              {{ data.description || '—' }}
            </span>
          </template>
        </Column>

        <Column header="Schemas" style="width: 10%">
          <template #body="{ data }">
            <div class="schema-badges">
              <Tag
                v-for="sv in data.specVersions"
                :key="sv.version"
                :value="sv.version"
                :severity="getSchemaStatusSeverity(sv.status)"
                v-tooltip.top="sv.status"
              />
              <span v-if="data.specVersions.length === 0" class="no-schemas"> No schemas </span>
            </div>
          </template>
        </Column>

        <Column header="Status" style="width: 10%">
          <template #body="{ data }">
            <Tag
              :value="data.status"
              :severity="data.status === 'CURRENT' ? 'success' : 'secondary'"
            />
          </template>
        </Column>

        <Column style="width: 5%">
          <template #body="{ data }">
            <div class="action-buttons">
              <Button
                icon="pi pi-pencil"
                rounded
                text
                severity="secondary"
                v-tooltip.left="'Edit'"
                @click.stop="viewEventType(data, true)"
              />
            </div>
          </template>
        </Column>

        <template #empty>
          <div class="empty-message">
            <i class="pi pi-inbox"></i>
            <span>No event types found</span>
            <Button v-if="hasActiveFilters" label="Clear filters" link @click="clearAll" />
          </div>
        </template>
      </DataTable>
    </div>

    <!-- Drawer outlet: detail/create child routes render over this list -->
    <RouterView v-slot="{ Component }">
      <component :is="Component" @changed="loadEventTypes" />
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
  justify-content: flex-end;
}

.table-card {
  padding: 0;
  overflow: hidden;
}

.name-text {
  font-weight: 500;
  color: #1e293b;
}

.description-text {
  color: #64748b;
  font-size: 13px;
  display: block;
  max-width: 250px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.schema-badges {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.no-schemas {
  color: #94a3b8;
  font-size: 12px;
  font-style: italic;
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

:deep(.clickable-row) {
  cursor: pointer;
  transition: background-color 0.15s;
}

:deep(.clickable-row:hover) {
  background-color: #f1f5f9 !important;
}

:deep(.p-datatable .p-datatable-thead > tr > th) {
  background: #f8fafc;
  color: #475569;
  font-weight: 600;
  font-size: 13px;
  text-transform: uppercase;
  letter-spacing: 0.025em;
}
</style>
