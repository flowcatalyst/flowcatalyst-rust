<script setup lang="ts">
// One claimed zone: its details, the public routes on its hostname, and
// Release. Release is refused (409 DOMAIN_IN_USE) while a live manifest
// still routes into it; the code is shown as the platform sends it.
import { computed, ref, watch } from "vue";
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
import { useClientOptions } from "@/composables/useClientOptions";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";
import { formatDate } from "@/pages/functions/format";

const emit = defineEmits<{
	changed: [];
}>();

const confirm = useConfirm();
const authStore = useAuthStore();
const clientOptions = useClientOptions();

const canManage = computed(() =>
	userHasPermission(authStore.user, "platform:function:domain:manage"),
);

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { id: hostname, goToList } = useDrawerRoute({
	listPath: "/function-domains",
	paramKey: "hostname",
});

const loading = ref(true);
const loadError = ref<string | null>(null);
const domain = ref<DomainResponse | null>(null);
const routes = ref<FunctionRouteResponse[]>([]);
const routesLoaded = ref(false);
const releaseError = ref<string | null>(null);

// Reactive param: the drawer instance is reused when switching between rows.
watch(
	hostname,
	async (value) => {
		if (!value) return;
		releaseError.value = null;
		await load(value);
	},
	{ immediate: true },
);

async function load(name: string) {
	loading.value = true;
	loadError.value = null;
	routesLoaded.value = false;
	try {
		domain.value = await functionsApi.getDomain(name);
		if (domain.value.owner !== "platform") void clientOptions.ensureLoaded().catch(() => {});
		void loadRoutes(name);
	} catch {
		domain.value = null;
		loadError.value = "Domain not found";
	} finally {
		loading.value = false;
	}
}

async function loadRoutes(name: string) {
	try {
		routes.value = await functionsApi.listRoutes({ hostname: name });
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
		emit("changed");
		void drawer.value?.close(true);
	} catch (e) {
		releaseError.value =
			e instanceof ApiError ? `${e.code ?? ""} ${e.message}`.trim() : "Release failed";
	}
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    :title="domain?.hostname || hostname || 'Domain'"
    subtitle="Function domain"
    size="wide"
    :loading="loading"
    :error="loadError"
    @close="goToList()"
  >
    <template v-if="domain">
      <FcFormSection title="Details" flat>
        <div class="fc-detail-grid">
          <FcDetailField label="Hostname">
            <code>{{ domain.hostname }}</code>
          </FcDetailField>
          <FcDetailField label="Owner" :value="ownerLabel" />
          <FcDetailField label="Claimed" :value="formatDate(domain.createdAt)" />
        </div>
      </FcFormSection>

      <FcFormSection title="Routes on this hostname" flat>
        <ProgressSpinner v-if="!routesLoaded" style="width: 24px; height: 24px" />
        <p v-else-if="routes.length === 0" class="text-muted text-sm">
          No live manifest routes to this hostname.
        </p>
        <DataTable v-else :value="routes" size="small">
          <Column header="Function">
            <template #body="{ data }">
              <RouterLink
                :to="`/functions/${encodeURIComponent(data.address)}`"
                class="font-mono text-sm"
              >
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
      </FcFormSection>

      <FcFormSection v-if="canManage" title="Actions" flat>
        <Message v-if="releaseError" severity="error" :closable="false" class="release-error">
          {{ releaseError }}
        </Message>
        <div class="action-item">
          <div class="action-info">
            <strong>Release Domain</strong>
            <p>Frees this zone. Refused while a live manifest still routes into it.</p>
          </div>
          <Button
            label="Release"
            icon="pi pi-trash"
            severity="danger"
            outlined
            @click="confirmRelease"
          />
        </div>
      </FcFormSection>
    </template>
  </EntityDrawer>
</template>

<style scoped>
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
  background: #fafafa;
  border-radius: 8px;
  border: 1px solid #e5e7eb;
}

.action-info strong {
  display: block;
  margin-bottom: 4px;
}

.action-info p {
  margin: 0;
  font-size: 13px;
  color: #64748b;
}
</style>
