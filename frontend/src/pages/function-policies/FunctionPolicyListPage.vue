<script setup lang="ts">
// Function policies (anchor only): per owner, the signers allowed to publish
// and the limit ceilings. `GET /api/function-policies` returns the stored
// rows; an owner without one is shown with the platform defaults, read once
// (the default shape is the same for every such owner).
import { computed, onMounted, ref } from "vue";
import { functionsApi, type PolicyResponse } from "@/api/functions";
import { useReturnTo } from "@/composables/useReturnTo";
import { useClientOptions } from "@/composables/useClientOptions";
import { useListState } from "@/composables/useListState";
import { formatDate } from "@/pages/functions/format";

const { navigateToDetail } = useReturnTo();
const clientOptions = useClientOptions();

interface Row {
	owner: string;
	ownerLabel: string;
	policy: PolicyResponse;
}

const rows = ref<Row[]>([]);
const loading = ref(true);

const { filters, hasActiveFilters, clearFilters } = useListState({
	filters: {
		q: { type: "string", key: "q" },
		stored: { type: "boolean", key: "stored" },
	},
});

const storedOptions = [
	{ label: "Custom policies", value: true },
	{ label: "Platform defaults", value: false },
];

const filteredRows = computed(() => {
	let result = rows.value;
	if (filters.stored.value !== null) {
		result = result.filter((r) => r.policy.stored === filters.stored.value);
	}
	const q = filters.q.value.trim().toLowerCase();
	if (q) {
		result = result.filter(
			(r) =>
				r.ownerLabel.toLowerCase().includes(q) ||
				r.owner.toLowerCase().includes(q) ||
				r.policy.signers.some(
					(s) => s.issuer.toLowerCase().includes(q) || s.subject.toLowerCase().includes(q),
				),
		);
	}
	return result;
});

async function load() {
	loading.value = true;
	try {
		const [, listResponse] = await Promise.all([
			clientOptions.ensureLoaded(),
			functionsApi.listPolicies(),
		]);
		const owners = [
			{ id: "platform", label: "Platform" },
			...clientOptions.options.value.map((c) => ({ id: c.value, label: c.label })),
		];
		const stored = new Map(listResponse.policies.map((p) => [p.owner, p]));
		const firstMissing = owners.find((o) => !stored.has(o.id));
		const defaults = firstMissing
			? await functionsApi.getPolicy(firstMissing.id).catch(() => null)
			: null;
		rows.value = owners.flatMap((o) => {
			const policy = stored.get(o.id) ?? (defaults ? { ...defaults, owner: o.id } : undefined);
			return policy ? [{ owner: o.id, ownerLabel: o.label, policy }] : [];
		});
	} catch (err) {
		console.error("Failed to load function policies", err);
		rows.value = [];
	} finally {
		loading.value = false;
	}
}

onMounted(load);

function viewPolicy(row: Row) {
	navigateToDetail(`/function-policies/${encodeURIComponent(row.owner)}`);
}

function onRowClick(event: { data: Row }) {
	viewPolicy(event.data);
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Function Policies</h1>
        <p class="page-subtitle">Per-owner signer allow-list and limit ceilings for publishing functions</p>
      </div>
    </header>

    <div class="fc-card">
      <div class="toolbar">
        <div class="filter-row">
          <IconField class="search-field">
            <InputIcon class="pi pi-search" />
            <InputText v-model="filters.q.value" placeholder="Owner, issuer or subject…" />
          </IconField>
          <Select
            v-model="filters.stored.value"
            :options="storedOptions"
            optionLabel="label"
            optionValue="value"
            placeholder="All owners"
            class="filter-select"
            showClear
          />
          <Button
            v-if="hasActiveFilters"
            icon="pi pi-filter-slash"
            text
            rounded
            severity="secondary"
            v-tooltip="'Clear filters'"
            @click="clearFilters"
          />
          <Button icon="pi pi-refresh" text rounded v-tooltip="'Refresh'" @click="load" />
        </div>
      </div>

      <DataTable
        :value="filteredRows"
        :loading="loading"
        data-key="owner"
        paginator
        :rows="50"
        :rowsPerPageOptions="[50, 100, 250]"
        row-hover
        selection-mode="single"
        stripedRows
        emptyMessage="No owners found"
        @row-click="onRowClick"
      >
        <Column header="Owner">
          <template #body="{ data }">
            <span :class="{ 'scope-platform': data.owner === 'platform' }">{{ data.ownerLabel }}</span>
          </template>
        </Column>
        <Column header="Signers">
          <template #body="{ data }">
            <span v-if="data.policy.signers.length === 0" class="text-muted">—</span>
            <div v-for="(s, idx) in data.policy.signers" v-else :key="idx" class="signer-line">
              {{ s.issuer }} / {{ s.subject }}
              <span class="text-muted">({{ s.runtimes.join(", ") }})</span>
            </div>
          </template>
        </Column>
        <Column header="Duration Ceiling">
          <template #body="{ data }">{{ data.policy.ceilings.maxDurationMs }} ms</template>
        </Column>
        <Column header="Concurrency Ceiling">
          <template #body="{ data }">{{ data.policy.ceilings.maxConcurrency }}</template>
        </Column>
        <Column header="Updated">
          <template #body="{ data }">
            <span class="text-sm">{{ formatDate(data.policy.updatedAt) }}</span>
          </template>
        </Column>
        <Column header="">
          <template #body="{ data }">
            <Tag
              :value="data.policy.stored ? 'custom' : 'platform defaults'"
              :severity="data.policy.stored ? 'info' : 'secondary'"
            />
          </template>
        </Column>
      </DataTable>
    </div>
  </div>
</template>

<style scoped>
.toolbar {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  margin-bottom: 16px;
}

.filter-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  flex-wrap: wrap;
}

.filter-select {
  min-width: 200px;
}

.search-field {
  flex: 1 1 240px;
}

.search-field :deep(.p-inputtext) {
  width: 100%;
}

.signer-line {
  font-size: 0.8125rem;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.text-sm {
  font-size: 0.875rem;
}

.text-muted {
  color: var(--text-color-secondary);
}

.scope-platform {
  font-style: italic;
}
</style>
