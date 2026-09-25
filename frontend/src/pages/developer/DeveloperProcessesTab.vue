<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { processesApi, type Process } from "@/api/processes";
import { toast } from "@/utils/errorBus";

// Processes relate to an Application by CODE (the leading segment of the
// colon-separated process code), not by id — mirrors ProcessListPage's own
// "application" filter param, so no backend change was needed for this tab.
const props = defineProps<{ applicationCode: string }>();

const router = useRouter();

const processes = ref<Process[]>([]);
const loading = ref(true);

const subdomain = ref<string | null>(null);
const status = ref<string | null>("CURRENT");

async function load() {
  loading.value = true;
  try {
    const res = await processesApi.list({ application: props.applicationCode });
    processes.value = res.items;
  } catch (e) {
    toast.error("Failed to load", (e as Error).message);
  } finally {
    loading.value = false;
  }
}

onMounted(load);
watch(() => props.applicationCode, load);

const subdomainOptions = computed(() => {
  const set = new Set(processes.value.map((p) => p.subdomain));
  return [...set].sort().map((v) => ({ label: v, value: v }));
});

const statusOptions = [
  { label: "Current", value: "CURRENT" },
  { label: "Archived", value: "ARCHIVED" },
];

const filtered = computed(() => {
  return processes.value.filter((p) => {
    if (subdomain.value && p.subdomain !== subdomain.value) return false;
    if (status.value && p.status !== status.value) return false;
    return true;
  });
});

const hasActiveFilters = computed(
  () => !!subdomain.value || status.value !== "CURRENT",
);

function clearFilters() {
  subdomain.value = null;
  status.value = "CURRENT";
}

function viewProcess(p: Process) {
  void router.push(`/processes/${p.id}`);
}

function editProcess(p: Process) {
  void router.push(`/processes/${p.id}/edit`);
}
</script>

<template>
  <div class="processes-tab">
    <div class="toolbar">
      <div class="filter-row">
        <Select
          v-model="subdomain"
          :options="subdomainOptions"
          option-label="label"
          option-value="value"
          placeholder="Subdomain"
          class="filter-select"
          show-clear
        />
        <Select
          v-model="status"
          :options="statusOptions"
          option-label="label"
          option-value="value"
          placeholder="Status"
          class="filter-select"
          show-clear
        />
        <Button
          v-if="hasActiveFilters"
          label="Clear filters"
          icon="pi pi-times"
          severity="secondary"
          text
          @click="clearFilters"
        />
      </div>
    </div>

    <div class="fc-card table-card">
      <DataTable
        :value="filtered"
        :loading="loading"
        size="small"
        rowHover
        :rowClass="() => 'clickable-row'"
        @row-click="(e) => viewProcess(e.data)"
      >
        <Column header="Code" style="width: 30%">
          <template #body="{ data }">
            <div class="code-display">
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
              {{ data.description || "—" }}
            </span>
          </template>
        </Column>

        <Column header="Tags" style="width: 12%">
          <template #body="{ data }">
            <div class="tag-list">
              <Tag v-for="t in data.tags" :key="t" :value="t" severity="secondary" />
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
            <Button
              icon="pi pi-pencil"
              text
              rounded
              severity="secondary"
              v-tooltip.left="'Edit'"
              @click.stop="editProcess(data)"
            />
          </template>
        </Column>

        <template #empty>
          <div class="empty-message">
            <i class="pi pi-sitemap"></i>
            <span v-if="hasActiveFilters">No processes match the current filters.</span>
            <span v-else>No processes defined for this application yet.</span>
            <Button v-if="hasActiveFilters" label="Clear filters" link @click="clearFilters" />
          </div>
        </template>
      </DataTable>
    </div>
  </div>
</template>

<style scoped>
.processes-tab {
  display: flex;
  flex-direction: column;
  gap: 16px;
  margin-top: 16px;
}

.toolbar {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.filter-row {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  align-items: center;
}

.filter-select {
  min-width: 180px;
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

.code-segment.subdomain {
  color: var(--text-color);
}

.code-segment.name {
  color: var(--text-color);
  font-weight: 500;
}

.code-separator {
  color: var(--text-color-secondary);
  margin: 0 2px;
}

.name-text {
  font-weight: 500;
}

.description-text {
  color: var(--text-color-secondary);
  font-size: 13px;
  display: block;
  max-width: 320px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tag-list {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.muted {
  color: var(--text-color-secondary);
  font-size: 12px;
}

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
