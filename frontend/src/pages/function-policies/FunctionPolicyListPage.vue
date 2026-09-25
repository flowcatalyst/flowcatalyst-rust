<script setup lang="ts">
// Function policies (anchor only): per owner, the signers allowed to publish
// and the limit ceilings. `GET /api/function-policies` returns the stored
// rows; an owner without one is shown with the platform defaults, read once
// (the default shape is the same for every such owner). A policy opens in a
// drawer over this list.
import { computed, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { functionsApi, type PolicyResponse } from "@/api/functions";
import { useClientOptions } from "@/composables/useClientOptions";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";
import { formatDate } from "@/pages/functions/format";

const router = useRouter();
const route = useRoute();
const clientOptions = useClientOptions();

interface Row {
	owner: string;
	ownerLabel: string;
	policy: PolicyResponse;
}

const rows = ref<Row[]>([]);
const loading = ref(true);

const listState = useListState({
	filters: {
		q: { type: "string", key: "q" },
		stored: { type: "boolean", key: "stored" },
	},
});
const { filters } = listState;
// The rows are filtered below (search spans signers); the toolbar only needs
// the popup badge and Clear All.
const { activeFilterCount, clearAll } = useTableFilters(listState, [
	{ field: "stored", param: "stored" },
]);

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

function openDetail(row: Row) {
	void router.push({
		path: `/function-policies/${encodeURIComponent(row.owner)}`,
		query: route.query,
	});
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
      <DataTable
        :value="filteredRows"
        :loading="loading"
        data-key="owner"
        paginator
        :rows="50"
        :rowsPerPageOptions="[50, 100, 250]"
        rowHover
        stripedRows
        :rowClass="() => 'clickable-row'"
        emptyMessage="No owners found"
        @row-click="(e) => openDetail(e.data)"
      >
        <template #header>
          <FcTableToolbar
            v-model:search="filters.q.value"
            search-placeholder="Owner, issuer or subject..."
            show-refresh
            :active-filter-count="activeFilterCount"
            :has-active-filters="listState.hasActiveFilters.value"
            @clear-all="clearAll"
            @refresh="load"
          >
            <template #filters>
              <FcFormField label="Policy">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.stored.value"
                    :options="storedOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All owners"
                    showClear
                    appendTo="self"
                  />
                </template>
              </FcFormField>
            </template>
          </FcTableToolbar>
        </template>
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

    <!-- Drawer outlet: the policy child route renders over this list -->
    <RouterView v-slot="{ Component }">
      <component :is="Component" @changed="load" />
    </RouterView>
  </div>
</template>

<style scoped>
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
