<script setup lang="ts">
// Function domains: zones claimed for functions' public routes. A claim
// covers its hostname and every hostname under it, and is usable as soon as
// it is made (no DNS verification step). `GET /api/function-domains` is
// owner-scoped (`clientId` is required), so an unscoped user picks an owner.
// Claim and detail open in a drawer over this list.
import { computed, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { functionsApi, type DomainResponse } from "@/api/functions";
import { useAuthStore } from "@/stores/auth";
import { isUnscopedUser, userHasPermission } from "@/stores/permissions";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";
import { useClientOptions } from "@/composables/useClientOptions";
import { formatDate } from "@/pages/functions/format";

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();
const clientOptions = useClientOptions();

const unscoped = computed(() => isUnscopedUser(authStore.user));
const canManage = computed(() =>
	userHasPermission(authStore.user, "platform:function:domain:manage"),
);

const listState = useListState(
	{
		filters: {
			q: { type: "string", key: "q" },
			owner: {
				type: "string",
				key: "owner",
				default: unscoped.value ? "platform" : (authStore.user?.clientId ?? "platform"),
			},
		},
	},
	() => load(),
);
const { filters } = listState;
// Client-side quick search over the loaded domains; the owner is a server
// query (the list is per owner), so it isn't a table filter.
const { filters: tableFilters } = useTableFilters(listState, []);

const ownerOptions = computed(() => [
	{ label: "Platform", value: "platform" },
	...clientOptions.options.value,
]);

const domains = ref<DomainResponse[]>([]);
const loading = ref(false);
let loadedOwner: string | null = null;

async function load() {
	const owner = filters.owner.value || "platform";
	// The quick search changes the URL too; only an owner change refetches.
	if (owner === loadedOwner) return;
	await fetchDomains(owner);
}

async function fetchDomains(owner = filters.owner.value || "platform") {
	loading.value = true;
	try {
		domains.value = await functionsApi.listDomains(owner);
		loadedOwner = owner;
	} catch (err) {
		console.error("Failed to load function domains", err);
		domains.value = [];
	} finally {
		loading.value = false;
	}
}

onMounted(async () => {
	if (unscoped.value) void clientOptions.ensureLoaded().catch(() => {});
	await fetchDomains();
});

function ownerLabel(owner: string): string {
	return owner === "platform" ? "Platform" : clientOptions.getLabel(owner);
}

function openDetail(d: DomainResponse) {
	void router.push({
		path: `/function-domains/${encodeURIComponent(d.hostname)}`,
		query: route.query,
	});
}

function openClaim() {
	void router.push({ path: "/function-domains/new", query: route.query });
}

/** A claim can land under another owner than the one listed: show that one. */
function onChanged(owner?: string) {
	if (owner && filters.owner.value !== owner) filters.owner.value = owner;
	else void fetchDomains();
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Function Domains</h1>
        <p class="page-subtitle">
          Zones claimed for functions' public routes. A claim covers every hostname under it and is
          usable immediately.
        </p>
      </div>
      <Button v-if="canManage" label="Claim Domain" icon="pi pi-plus" @click="openClaim" />
    </header>

    <div class="fc-card">
      <DataTable
        :value="domains"
        :loading="loading"
        :filters="tableFilters"
        :globalFilterFields="['hostname']"
        data-key="id"
        rowHover
        stripedRows
        :rowClass="() => 'clickable-row'"
        emptyMessage="No domains claimed for this owner"
        @row-click="(e) => openDetail(e.data)"
      >
        <template #header>
          <FcTableToolbar
            v-model:search="filters.q.value"
            search-placeholder="Search domains..."
            show-refresh
            @refresh="fetchDomains()"
          >
            <template v-if="unscoped" #start>
              <Select
                v-model="filters.owner.value"
                :options="ownerOptions"
                optionLabel="label"
                optionValue="value"
                placeholder="Owner"
                class="owner-select"
                filter
              />
            </template>
          </FcTableToolbar>
        </template>
        <Column header="Domain" field="hostname" sortable>
          <template #body="{ data }">
            <span class="font-mono text-sm">{{ data.hostname }}</span>
          </template>
        </Column>
        <Column header="Owner">
          <template #body="{ data }">{{ ownerLabel(data.owner) }}</template>
        </Column>
        <Column header="Claimed" field="createdAt" sortable>
          <template #body="{ data }">
            <span class="text-sm">{{ formatDate(data.createdAt) }}</span>
          </template>
        </Column>
      </DataTable>
    </div>

    <!-- Drawer outlet: claim/detail child routes render over this list -->
    <RouterView v-slot="{ Component }">
      <component :is="Component" @changed="onChanged" />
    </RouterView>
  </div>
</template>

<style scoped>
.owner-select {
  min-width: 200px;
}

.font-mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.text-sm {
  font-size: 0.875rem;
}
</style>
