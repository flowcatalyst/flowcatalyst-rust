<script setup lang="ts">
// Function detail: Overview (details, hosts, wiring, actions), Versions
// (with aliases), Config & Secrets, Public Routes and Invoke. The last four
// are their own components so each can be tested without this page.
import { computed, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useConfirm } from "primevue/useconfirm";
import { toast } from "@/utils/errorBus";
import {
	functionsApi,
	type FunctionResponse,
	type StatusResponse,
	type VersionResponse,
} from "@/api/functions";
import { useAuthStore } from "@/stores/auth";
import { userHasPermission } from "@/stores/permissions";
import { useReturnTo } from "@/composables/useReturnTo";
import { useClientOptions } from "@/composables/useClientOptions";
import FunctionVersionsTab from "./FunctionVersionsTab.vue";
import FunctionConfigSecretsTab from "./FunctionConfigSecretsTab.vue";
import FunctionPublicRoutesTab from "./FunctionPublicRoutesTab.vue";
import FunctionInvokeTab from "./FunctionInvokeTab.vue";
import {
	formatDate,
	functionStatusSeverity,
	heartbeatAge,
	loadedStateSeverity,
} from "./format";

const TABS = ["overview", "versions", "config", "routes", "invoke"] as const;
type TabName = (typeof TABS)[number];

const route = useRoute();
const router = useRouter();
const confirm = useConfirm();
const authStore = useAuthStore();
const { returnTo } = useReturnTo();
const clientOptions = useClientOptions();

const canManage = computed(() =>
	userHasPermission(authStore.user, "platform:function:function:manage"),
);
const canInvoke = computed(() =>
	userHasPermission(authStore.user, "platform:function:version:invoke"),
);

const address = computed(() => route.params["address"] as string);

const loading = ref(true);
const fn = ref<FunctionResponse | null>(null);
const status = ref<StatusResponse | null>(null);
const statusLoading = ref(false);

const editing = ref(false);
const editDescription = ref("");
const saving = ref(false);

const invokeVersions = ref<VersionResponse[]>([]);
const invokeVersionsLoaded = ref(false);

function tabFromQuery(): TabName {
	const tab = route.query["tab"];
	return typeof tab === "string" && (TABS as readonly string[]).includes(tab)
		? (tab as TabName)
		: "overview";
}

const activeTab = ref<TabName>(tabFromQuery());

// Keep the tab in the URL (replace, not push) so a reload or a return from
// the manifest editor lands on the same tab.
watch(activeTab, (tab) => {
	const query = { ...route.query };
	if (tab === "overview") delete query["tab"];
	else query["tab"] = tab;
	void router.replace({ query });
	if (tab === "invoke") void loadInvokeVersions();
});

/** `?highlight=<n>` from the manifest editor's publish: the new version row. */
const highlightVersion = computed(() => {
	const raw = route.query["highlight"];
	const n = typeof raw === "string" ? Number(raw) : NaN;
	return Number.isInteger(n) ? n : null;
});

watch(
	address,
	async (value) => {
		if (!value) return;
		editing.value = false;
		invokeVersionsLoaded.value = false;
		invokeVersions.value = [];
		await loadFunction(value);
		void loadStatus(value);
		if (activeTab.value === "invoke") void loadInvokeVersions();
	},
	{ immediate: true },
);

async function loadFunction(addr: string) {
	loading.value = true;
	try {
		fn.value = await functionsApi.get(addr);
		if (fn.value.clientId) void clientOptions.ensureLoaded().catch(() => {});
	} catch {
		fn.value = null;
	} finally {
		loading.value = false;
	}
}

async function loadStatus(addr: string) {
	statusLoading.value = true;
	try {
		status.value = await functionsApi.status(addr);
	} catch {
		status.value = null;
	} finally {
		statusLoading.value = false;
	}
}

async function loadInvokeVersions() {
	if (invokeVersionsLoaded.value || !fn.value) return;
	invokeVersionsLoaded.value = true;
	try {
		const versions = await functionsApi.listVersions(fn.value.address);
		invokeVersions.value = [...versions].sort((a, b) => b.version - a.version);
	} catch {
		invokeVersions.value = [];
	}
}

/** A promote, retire or publish can change the live alias and the hosts' reports. */
async function onVersionsChanged() {
	if (!fn.value) return;
	invokeVersionsLoaded.value = false;
	await loadFunction(fn.value.address);
	void loadStatus(fn.value.address);
}

function ownerLabel(): string {
	if (!fn.value?.clientId) return "Platform";
	return clientOptions.getLabel(fn.value.clientId);
}

function startEditing() {
	if (!fn.value) return;
	editDescription.value = fn.value.description ?? "";
	editing.value = true;
}

async function saveChanges() {
	if (!fn.value) return;
	saving.value = true;
	const addr = fn.value.address;
	try {
		await functionsApi.update(addr, { description: editDescription.value });
		await loadFunction(addr);
		editing.value = false;
		toast.success("Success", "Function updated");
	} catch {
		// surfaced by the global error toast
	} finally {
		saving.value = false;
	}
}

function confirmToggleStatus() {
	if (!fn.value) return;
	const disabling = fn.value.status === "ACTIVE";
	confirm.require({
		message: disabling
			? `Disable ${fn.value.address}? Hosts unload it and new publishes are refused until it is enabled again.`
			: `Enable ${fn.value.address}?`,
		header: disabling ? "Disable Function" : "Enable Function",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: disabling ? "Disable" : "Enable",
		acceptClass: disabling ? "p-button-warning" : undefined,
		accept: () => setStatus(disabling ? "DISABLED" : "ACTIVE"),
	});
}

async function setStatus(next: "ACTIVE" | "DISABLED") {
	if (!fn.value) return;
	const addr = fn.value.address;
	try {
		await functionsApi.update(addr, { status: next });
		await loadFunction(addr);
		void loadStatus(addr);
		toast.success("Success", next === "ACTIVE" ? "Function enabled" : "Function disabled");
	} catch {
		// surfaced by the global error toast
	}
}

function confirmDelete() {
	if (!fn.value) return;
	confirm.require({
		message:
			`Delete ${fn.value.address}? This permanently deletes the function and cascades to ` +
			"its versions, aliases, routes, trigger objects and stored artifacts. This cannot be undone.",
		header: "Delete Function",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Delete",
		acceptClass: "p-button-danger",
		accept: deleteFunction,
	});
}

async function deleteFunction() {
	if (!fn.value) return;
	try {
		await functionsApi.delete(fn.value.address);
		toast.success("Success", "Function deleted");
		returnTo("/functions");
	} catch {
		// surfaced by the global error toast
	}
}
</script>

<template>
  <div class="page-container">
    <div v-if="loading && !fn" class="loading-container">
      <ProgressSpinner strokeWidth="3" />
    </div>

    <template v-else-if="fn">
      <header class="page-header">
        <div class="header-content">
          <Button
            icon="pi pi-arrow-left"
            text
            severity="secondary"
            v-tooltip="'Back to list'"
            @click="returnTo('/functions')"
          />
          <div class="header-text">
            <h1 class="page-title">{{ fn.name }}</h1>
            <code class="fn-address">{{ fn.address }}</code>
          </div>
          <Tag :value="fn.status" :severity="functionStatusSeverity(fn.status)" />
        </div>
      </header>

      <Tabs v-model:value="activeTab">
        <TabList>
          <Tab value="overview">Overview</Tab>
          <Tab value="versions">Versions</Tab>
          <Tab value="config">Config &amp; Secrets</Tab>
          <Tab value="routes">Public Routes</Tab>
          <Tab v-if="canInvoke" value="invoke">Invoke</Tab>
        </TabList>
        <TabPanels>
          <TabPanel value="overview">
            <div class="section-card">
              <div class="card-header">
                <h3>Function Details</h3>
                <Button
                  v-if="!editing && canManage"
                  icon="pi pi-pencil"
                  label="Edit"
                  text
                  @click="startEditing"
                />
              </div>
              <div class="card-content">
                <template v-if="editing">
                  <div class="form-field">
                    <label for="fn-edit-description">Description</label>
                    <Textarea
                      id="fn-edit-description"
                      v-model="editDescription"
                      class="full-width"
                      rows="3"
                    />
                  </div>
                  <div class="form-actions">
                    <Button label="Cancel" severity="secondary" outlined @click="editing = false" />
                    <Button label="Save" :loading="saving" @click="saveChanges" />
                  </div>
                </template>
                <div v-else class="detail-grid">
                  <div class="detail-item">
                    <label>Address</label>
                    <code>{{ fn.address }}</code>
                  </div>
                  <div class="detail-item">
                    <label>Owner</label>
                    <span>{{ ownerLabel() }}</span>
                  </div>
                  <div class="detail-item">
                    <label>Runtime</label>
                    <span>{{ fn.runtime }}</span>
                  </div>
                  <div class="detail-item">
                    <label>Live Version</label>
                    <span v-if="fn.live">v{{ fn.live.version }}</span>
                    <span v-else class="text-muted">— (no live version yet)</span>
                  </div>
                  <div class="detail-item full-width">
                    <label>Description</label>
                    <span v-if="fn.description">{{ fn.description }}</span>
                    <span v-else class="text-muted">—</span>
                  </div>
                  <div class="detail-item">
                    <label>Created</label>
                    <span>{{ formatDate(fn.createdAt) }}</span>
                  </div>
                  <div class="detail-item">
                    <label>Updated</label>
                    <span>{{ formatDate(fn.updatedAt) }}</span>
                  </div>
                </div>
              </div>
            </div>

            <div class="section-card">
              <div class="card-header">
                <h3>Hosts</h3>
                <Button
                  icon="pi pi-refresh"
                  text
                  rounded
                  v-tooltip="'Refresh'"
                  @click="loadStatus(fn.address)"
                />
              </div>
              <div class="card-content">
                <ProgressSpinner v-if="statusLoading" style="width: 24px; height: 24px" />
                <p v-else-if="!status || status.hosts.length === 0" class="text-muted text-sm">
                  No host has reported this function yet.
                </p>
                <DataTable v-else :value="status.hosts" data-key="hostId" size="small">
                  <Column header="Host">
                    <template #body="{ data }">
                      <span class="font-mono text-sm">{{ data.hostId }}</span>
                    </template>
                  </Column>
                  <Column header="Pool" field="pool" />
                  <Column header="State">
                    <template #body="{ data }">
                      <Tag
                        :value="data.state"
                        :severity="data.state === 'ACTIVE' ? 'success' : 'warn'"
                      />
                      <span v-if="data.stale" class="stale-flag">stale</span>
                    </template>
                  </Column>
                  <Column header="Last Heartbeat">
                    <template #body="{ data }">
                      <span v-tooltip="formatDate(data.lastHeartbeat)">
                        {{ heartbeatAge(data.lastHeartbeat) }}
                      </span>
                    </template>
                  </Column>
                  <Column header="Versions">
                    <template #body="{ data }">
                      <div class="loaded-versions">
                        <Tag
                          v-for="loaded in data.loaded"
                          :key="loaded.version"
                          v-tooltip="loaded.error"
                          :value="`v${loaded.version} ${loaded.state}`"
                          :severity="loadedStateSeverity(loaded.state)"
                        />
                        <span v-if="data.loaded.length === 0" class="text-muted">—</span>
                      </div>
                    </template>
                  </Column>
                </DataTable>
              </div>
            </div>

            <div v-if="status && status.wiring.length > 0" class="section-card">
              <div class="card-header">
                <h3>Wiring</h3>
              </div>
              <div class="card-content">
                <p class="text-muted text-sm wiring-intro">
                  The platform objects the live manifest created. A missing one was deleted by hand.
                </p>
                <DataTable :value="status.wiring" data-key="objectId" size="small">
                  <Column header="Kind" field="kind" />
                  <Column header="Code">
                    <template #body="{ data }">
                      <span class="font-mono text-sm">{{ data.code }}</span>
                    </template>
                  </Column>
                  <Column header="Object">
                    <template #body="{ data }">
                      <span class="font-mono text-sm">{{ data.objectId }}</span>
                    </template>
                  </Column>
                  <Column header="Present">
                    <template #body="{ data }">
                      <Tag
                        :value="data.present ? 'present' : 'missing'"
                        :severity="data.present ? 'success' : 'danger'"
                      />
                    </template>
                  </Column>
                </DataTable>
              </div>
            </div>

            <div v-if="canManage && !editing" class="section-card">
              <div class="card-header">
                <h3>Actions</h3>
              </div>
              <div class="card-content">
                <div class="action-items">
                  <div class="action-item">
                    <div class="action-info">
                      <strong>{{ fn.status === "ACTIVE" ? "Disable Function" : "Enable Function" }}</strong>
                      <p v-if="fn.status === 'ACTIVE'">
                        Stops every host serving it; the function and its versions are kept.
                      </p>
                      <p v-else>Lets hosts load its live version again.</p>
                    </div>
                    <Button
                      :label="fn.status === 'ACTIVE' ? 'Disable' : 'Enable'"
                      :severity="fn.status === 'ACTIVE' ? 'warn' : 'success'"
                      outlined
                      @click="confirmToggleStatus"
                    />
                  </div>
                  <div class="action-item">
                    <div class="action-info">
                      <strong>Delete Function</strong>
                      <p>Permanently deletes this function and everything published under it.</p>
                    </div>
                    <Button
                      label="Delete"
                      icon="pi pi-trash"
                      severity="danger"
                      outlined
                      @click="confirmDelete"
                    />
                  </div>
                </div>
              </div>
            </div>
          </TabPanel>

          <TabPanel value="versions">
            <FunctionVersionsTab
              :address="fn.address"
              :highlight-version="highlightVersion"
              @changed="onVersionsChanged"
            />
          </TabPanel>

          <TabPanel value="config">
            <FunctionConfigSecretsTab :address="fn.address" :live-version="fn.live?.version" />
          </TabPanel>

          <TabPanel value="routes">
            <FunctionPublicRoutesTab
              :address="fn.address"
              :client-id="fn.clientId"
              :has-live-version="!!fn.live"
            />
          </TabPanel>

          <TabPanel v-if="canInvoke" value="invoke">
            <FunctionInvokeTab
              :address="fn.address"
              :versions="invokeVersions"
              :live-version="fn.live?.version"
            />
          </TabPanel>
        </TabPanels>
      </Tabs>
    </template>

    <Message v-else severity="error">Function not found</Message>
  </div>
</template>

<style scoped>
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

.header-text {
  flex: 1;
}

.fn-address {
  display: inline-block;
  margin-top: 4px;
  background: var(--surface-ground);
  padding: 4px 10px;
  border-radius: 4px;
  font-size: 14px;
}

.section-card {
  margin-top: 16px;
  background: var(--surface-card, white);
  border-radius: 8px;
  border: 1px solid var(--surface-border);
  overflow: hidden;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
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
  grid-template-columns: repeat(2, 1fr);
  gap: 20px;
}

.detail-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.detail-item.full-width {
  grid-column: 1 / -1;
}

.detail-item label {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-color-secondary);
  text-transform: uppercase;
}

.form-field {
  margin-bottom: 20px;
}

.form-field label {
  display: block;
  margin-bottom: 6px;
  font-weight: 500;
}

.full-width {
  width: 100%;
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: 12px;
  padding-top: 16px;
  border-top: 1px solid var(--surface-border);
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

.stale-flag {
  margin-left: 6px;
  font-size: 11px;
  color: #b91c1c;
}

.loaded-versions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.wiring-intro {
  margin: 0 0 12px;
}

.action-items {
  display: flex;
  flex-direction: column;
  gap: 16px;
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

@media (max-width: 640px) {
  .detail-grid {
    grid-template-columns: 1fr;
  }
}
</style>
