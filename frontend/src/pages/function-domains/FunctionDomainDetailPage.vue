<script setup lang="ts">
// One claimed zone: its details, the public routes on its hostname, and
// Release. Release is refused (409 DOMAIN_IN_USE) while a live manifest
// still routes into it; the code is shown as the platform sends it.
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import { useConfirm } from "primevue/useconfirm";
import { toast } from "@/utils/errorBus";
import { ApiError } from "@/api/client";
import {
	functionsApi,
	type DomainResponse,
	type FunctionRouteResponse,
} from "@/api/functions";
import { useAuthStore } from "@/stores/auth";
import { userHasPermission } from "@/stores/permissions";
import { useReturnTo } from "@/composables/useReturnTo";
import { useClientOptions } from "@/composables/useClientOptions";
import { formatDate } from "@/pages/functions/format";

const route = useRoute();
const confirm = useConfirm();
const authStore = useAuthStore();
const { returnTo } = useReturnTo();
const clientOptions = useClientOptions();

const canManage = computed(() =>
	userHasPermission(authStore.user, "platform:function:domain:manage"),
);

const hostname = computed(() => route.params["hostname"] as string);
const loading = ref(true);
const domain = ref<DomainResponse | null>(null);
const routes = ref<FunctionRouteResponse[]>([]);
const routesLoaded = ref(false);
const releaseError = ref<string | null>(null);

onMounted(async () => {
	loading.value = true;
	try {
		domain.value = await functionsApi.getDomain(hostname.value);
		if (domain.value.owner !== "platform") void clientOptions.ensureLoaded().catch(() => {});
		void loadRoutes();
	} catch {
		domain.value = null;
	} finally {
		loading.value = false;
	}
});

async function loadRoutes() {
	try {
		routes.value = await functionsApi.listRoutes({ hostname: hostname.value });
	} catch {
		routes.value = [];
	} finally {
		routesLoaded.value = true;
	}
}

const ownerLabel = computed(() => {
	const owner = domain.value?.owner;
	if (!owner) return "";
	return owner === "platform" ? "Platform" : clientOptions.getLabel(owner);
});

function confirmRelease() {
	if (!domain.value) return;
	confirm.require({
		message:
			`Release ${domain.value.hostname}? This cannot be undone, and is refused while any ` +
			"live function manifest still routes to it.",
		header: "Release Domain",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Release",
		acceptClass: "p-button-danger",
		accept: release,
	});
}

async function release() {
	if (!domain.value) return;
	releaseError.value = null;
	try {
		await functionsApi.releaseDomain(domain.value.hostname, { suppressGlobalErrorToast: true });
		toast.success("Success", "Domain released");
		returnTo("/function-domains");
	} catch (e) {
		releaseError.value =
			e instanceof ApiError ? `${e.code ?? ""} ${e.message}`.trim() : "Release failed";
	}
}
</script>

<template>
  <div class="page-container">
    <div v-if="loading" class="loading-container">
      <ProgressSpinner strokeWidth="3" />
    </div>

    <template v-else-if="domain">
      <header class="page-header">
        <div class="header-content">
          <Button
            icon="pi pi-arrow-left"
            text
            severity="secondary"
            v-tooltip="'Back to list'"
            @click="returnTo('/function-domains')"
          />
          <div>
            <h1 class="page-title">{{ domain.hostname }}</h1>
            <p class="page-subtitle">Function domain</p>
          </div>
        </div>
      </header>

      <div class="section-card">
        <div class="card-header"><h3>Details</h3></div>
        <div class="card-content detail-grid">
          <div class="detail-item">
            <label>Hostname</label>
            <code>{{ domain.hostname }}</code>
          </div>
          <div class="detail-item">
            <label>Owner</label>
            <span>{{ ownerLabel }}</span>
          </div>
          <div class="detail-item">
            <label>Claimed</label>
            <span>{{ formatDate(domain.createdAt) }}</span>
          </div>
        </div>
      </div>

      <div class="section-card">
        <div class="card-header"><h3>Routes on this hostname</h3></div>
        <div class="card-content">
          <ProgressSpinner v-if="!routesLoaded" style="width: 24px; height: 24px" />
          <p v-else-if="routes.length === 0" class="text-muted text-sm">
            No live manifest routes to this hostname.
          </p>
          <DataTable v-else :value="routes" size="small">
            <Column header="Function">
              <template #body="{ data }">
                <RouterLink :to="`/functions/${encodeURIComponent(data.address)}`" class="font-mono text-sm">
                  {{ data.address }}
                </RouterLink>
              </template>
            </Column>
            <Column header="Path Prefix">
              <template #body="{ data }">
                <span class="font-mono text-sm">{{ data.pathPrefix }}</span>
              </template>
            </Column>
            <Column header="Alias Prefixes">
              <template #body="{ data }">
                {{ data.aliasPrefixes.length ? data.aliasPrefixes.join(", ") : "none" }}
              </template>
            </Column>
          </DataTable>
        </div>
      </div>

      <div v-if="canManage" class="section-card">
        <div class="card-header"><h3>Actions</h3></div>
        <div class="card-content">
          <Message v-if="releaseError" severity="error" :closable="false" class="release-error">
            {{ releaseError }}
          </Message>
          <div class="action-item">
            <div class="action-info">
              <strong>Release Domain</strong>
              <p>Frees this zone. Refused while a live manifest still routes into it.</p>
            </div>
            <Button label="Release" icon="pi pi-trash" severity="danger" outlined @click="confirmRelease" />
          </div>
        </div>
      </div>
    </template>

    <Message v-else severity="error">Domain not found</Message>
  </div>
</template>

<style scoped>
.page-container {
  max-width: 900px;
}

.loading-container {
  display: flex;
  justify-content: center;
  padding: 60px;
}

.header-content {
  display: flex;
  align-items: flex-start;
  gap: 16px;
}

.section-card {
  margin-bottom: 16px;
  background: var(--surface-card, white);
  border-radius: 8px;
  border: 1px solid var(--surface-border);
  overflow: hidden;
}

.card-header {
  padding: 12px 20px;
  border-bottom: 1px solid var(--surface-border);
}

.card-header h3 {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
}

.card-content {
  padding: 20px;
}

.detail-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 20px;
}

.detail-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.detail-item label {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-color-secondary);
  text-transform: uppercase;
}

.font-mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.text-sm {
  font-size: 0.875rem;
}

.text-muted {
  color: var(--text-color-secondary);
}

.release-error {
  margin-bottom: 12px;
}

.action-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  padding: 16px;
  background: var(--surface-ground);
  border-radius: 8px;
  border: 1px solid var(--surface-border);
}

.action-info strong {
  display: block;
  margin-bottom: 4px;
}

.action-info p {
  margin: 0;
  font-size: 13px;
  color: var(--text-color-secondary);
}
</style>
