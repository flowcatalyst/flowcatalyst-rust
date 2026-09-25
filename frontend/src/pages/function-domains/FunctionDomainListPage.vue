<script setup lang="ts">
// Function domains: zones claimed for functions' public routes. A claim
// covers its hostname and every hostname under it, and is usable as soon as
// it is made (no DNS verification step). `GET /api/function-domains` is
// owner-scoped (`clientId` is required), so an unscoped user picks an owner.
import { computed, onMounted, ref } from "vue";
import { toast } from "@/utils/errorBus";
import { ApiError } from "@/api/client";
import { functionsApi, type DomainResponse } from "@/api/functions";
import { useAuthStore } from "@/stores/auth";
import { isUnscopedUser, userHasPermission } from "@/stores/permissions";
import { useListState } from "@/composables/useListState";
import { useReturnTo } from "@/composables/useReturnTo";
import { useClientOptions } from "@/composables/useClientOptions";
import { formatDate } from "@/pages/functions/format";

const authStore = useAuthStore();
const { navigateToDetail } = useReturnTo();
const clientOptions = useClientOptions();

const unscoped = computed(() => isUnscopedUser(authStore.user));
const canManage = computed(() =>
	userHasPermission(authStore.user, "platform:function:domain:manage"),
);

const { filters } = useListState(
	{
		filters: {
			owner: {
				type: "string",
				key: "owner",
				default: unscoped.value ? "platform" : (authStore.user?.clientId ?? "platform"),
			},
		},
	},
	() => load(),
);

const ownerOptions = computed(() => [
	{ label: "Platform", value: "platform" },
	...clientOptions.options.value,
]);

const domains = ref<DomainResponse[]>([]);
const loading = ref(false);

async function load() {
	loading.value = true;
	try {
		domains.value = await functionsApi.listDomains(filters.owner.value || "platform");
	} catch (err) {
		console.error("Failed to load function domains", err);
		domains.value = [];
	} finally {
		loading.value = false;
	}
}

onMounted(async () => {
	if (unscoped.value) void clientOptions.ensureLoaded().catch(() => {});
	await load();
});

function ownerLabel(owner: string): string {
	return owner === "platform" ? "Platform" : clientOptions.getLabel(owner);
}

function viewDomain(d: DomainResponse) {
	navigateToDetail(`/function-domains/${encodeURIComponent(d.hostname)}`);
}

function onRowClick(event: { data: DomainResponse }) {
	viewDomain(event.data);
}

// ── Claim ───────────────────────────────────────────────────────────────────

const showClaimDialog = ref(false);
const claimHostname = ref("");
const claimPlatformOwned = ref(true);
const claimClientId = ref<string | null>(null);
const claiming = ref(false);
const claimError = ref<string | null>(null);

const claimValid = computed(
	() =>
		claimHostname.value.trim().length > 0 &&
		(!unscoped.value || claimPlatformOwned.value || !!claimClientId.value),
);

function openClaimDialog() {
	claimHostname.value = "";
	claimPlatformOwned.value = filters.owner.value === "platform";
	claimClientId.value = filters.owner.value === "platform" ? null : filters.owner.value;
	claimError.value = null;
	showClaimDialog.value = true;
}

async function submitClaim() {
	if (!claimValid.value) return;
	claiming.value = true;
	claimError.value = null;
	try {
		const clientId = unscoped.value
			? claimPlatformOwned.value
				? undefined
				: (claimClientId.value ?? undefined)
			: (authStore.user?.clientId ?? undefined);
		const domain = await functionsApi.claimDomain(
			{ hostname: claimHostname.value.trim(), clientId },
			{ suppressGlobalErrorToast: true },
		);
		toast.success("Success", `${domain.hostname} claimed`);
		showClaimDialog.value = false;
		if (filters.owner.value !== domain.owner) filters.owner.value = domain.owner;
		else await load();
	} catch (e) {
		claimError.value =
			e instanceof ApiError ? `${e.code ?? ""} ${e.message}`.trim() : "Failed to claim domain";
	} finally {
		claiming.value = false;
	}
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
      <Button v-if="canManage" label="Claim Domain" icon="pi pi-plus" @click="openClaimDialog" />
    </header>

    <div class="fc-card">
      <div class="toolbar">
        <div class="filter-row">
          <Select
            v-if="unscoped"
            v-model="filters.owner.value"
            :options="ownerOptions"
            optionLabel="label"
            optionValue="value"
            placeholder="Owner"
            class="filter-select"
            filter
          />
          <Button icon="pi pi-refresh" text rounded v-tooltip="'Refresh'" @click="load" />
        </div>
      </div>

      <DataTable
        :value="domains"
        :loading="loading"
        data-key="id"
        row-hover
        selection-mode="single"
        stripedRows
        emptyMessage="No domains claimed for this owner"
        @row-click="onRowClick"
      >
        <Column header="Domain">
          <template #body="{ data }">
            <span class="font-mono text-sm">{{ data.hostname }}</span>
          </template>
        </Column>
        <Column header="Owner">
          <template #body="{ data }">{{ ownerLabel(data.owner) }}</template>
        </Column>
        <Column header="Claimed">
          <template #body="{ data }">
            <span class="text-sm">{{ formatDate(data.createdAt) }}</span>
          </template>
        </Column>
        <Column header="" style="width: 4rem">
          <template #body="{ data }">
            <Button
              icon="pi pi-arrow-right"
              severity="secondary"
              text
              rounded
              @click.stop="viewDomain(data)"
            />
          </template>
        </Column>
      </DataTable>
    </div>

    <Dialog v-model:visible="showClaimDialog" header="Claim Domain" modal :style="{ width: '32rem' }">
      <div class="form-field">
        <label for="claim-hostname">Hostname <span class="required">*</span></label>
        <InputText
          id="claim-hostname"
          v-model="claimHostname"
          placeholder="functions.acme.com"
          class="full-width"
          autofocus
        />
        <small class="hint">Every hostname under it (e.g. <code>app.functions.acme.com</code>) is covered too.</small>
      </div>
      <template v-if="unscoped">
        <div class="form-field checkbox-field">
          <Checkbox v-model="claimPlatformOwned" :binary="true" inputId="claimPlatformOwned" />
          <label for="claimPlatformOwned">Platform-owned</label>
        </div>
        <div v-if="!claimPlatformOwned" class="form-field">
          <label>Client <span class="required">*</span></label>
          <ClientSelect v-model="claimClientId" placeholder="Search for a client" />
        </div>
      </template>
      <Message v-if="claimError" severity="error" :closable="false">{{ claimError }}</Message>
      <template #footer>
        <Button label="Cancel" text :disabled="claiming" @click="showClaimDialog = false" />
        <Button label="Claim" :loading="claiming" :disabled="!claimValid" @click="submitClaim" />
      </template>
    </Dialog>
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

.font-mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.text-sm {
  font-size: 0.875rem;
}

.form-field {
  margin-bottom: 16px;
}

.form-field > label {
  display: block;
  font-weight: 500;
  margin-bottom: 6px;
}

.checkbox-field {
  display: flex;
  align-items: center;
  gap: 8px;
}

.checkbox-field label {
  margin: 0;
  cursor: pointer;
}

.required {
  color: #ef4444;
}

.full-width {
  width: 100%;
}

.hint {
  display: block;
  margin-top: 4px;
  font-size: 12px;
  color: var(--text-color-secondary);
}
</style>
