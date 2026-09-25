<script setup lang="ts">
// Versions and aliases. Promote is enabled only for a READY version; Retire
// never for the live one (or one already retired). The manifest editor is
// its own page (`/functions/:address/manifest`), opened three ways: a new
// manifest, a version's manifest ("Edit as new version"), or an imported file.
import { computed, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useConfirm } from "primevue/useconfirm";
import { toast } from "@/utils/errorBus";
import { ApiError } from "@/api/client";
import {
	functionsApi,
	type AliasResponse,
	type Manifest,
	type PublishResponse,
	type VersionResponse,
} from "@/api/functions";
import { useAuthStore } from "@/stores/auth";
import { userHasPermission } from "@/stores/permissions";
import PublishVersionDialog from "./PublishVersionDialog.vue";
import { parseManifestText } from "./manifestModel";
import { setPendingManifestSeed } from "./manifestSeed";
import { DNS_LABEL_PATTERN, formatDate, shortDigest, versionStateSeverity } from "./format";

const props = defineProps<{
	address: string;
	/** A version to highlight on load (the one just published from the editor). */
	highlightVersion?: number | null;
}>();

const emit = defineEmits<{
	/** A publish, promote, retire or alias removal: the live alias or a state changed. */
	changed: [];
}>();

const router = useRouter();
const confirm = useConfirm();
const authStore = useAuthStore();

const canPublish = computed(() =>
	userHasPermission(authStore.user, "platform:function:version:publish"),
);
const canPromote = computed(() =>
	userHasPermission(authStore.user, "platform:function:alias:promote"),
);

const versions = ref<VersionResponse[]>([]);
const loading = ref(true);
const expandedRows = ref<Record<number, boolean>>({});
const manifestByVersion = ref<Record<number, Manifest | null>>({});
const highlighted = ref<number | null>(props.highlightVersion ?? null);

const aliases = ref<AliasResponse[]>([]);
const aliasesLoading = ref(true);

const showPublishDialog = ref(false);

watch(
	() => props.address,
	async (addr) => {
		if (!addr) return;
		await Promise.all([loadVersions(addr), loadAliases(addr)]);
	},
	{ immediate: true },
);

async function loadVersions(addr: string) {
	loading.value = true;
	try {
		const result = await functionsApi.listVersions(addr);
		// Newest first: a fresh candidate or the live version is what an
		// operator opens this tab for.
		versions.value = [...result].sort((a, b) => b.version - a.version);
	} catch {
		versions.value = [];
	} finally {
		loading.value = false;
	}
}

async function loadAliases(addr: string) {
	aliasesLoading.value = true;
	try {
		aliases.value = await functionsApi.listAliases(addr);
	} catch {
		aliases.value = [];
	} finally {
		aliasesLoading.value = false;
	}
}

async function onRowExpand(event: { data: VersionResponse }) {
	const v = event.data;
	if (manifestByVersion.value[v.version] !== undefined) return;
	try {
		const full = await functionsApi.getVersion(props.address, v.version);
		manifestByVersion.value[v.version] = full.manifest ?? null;
	} catch {
		manifestByVersion.value[v.version] = null;
	}
}

function prettyManifest(version: number): string {
	const manifest = manifestByVersion.value[version];
	if (manifest === undefined) return "Loading…";
	if (!manifest) return "—";
	return JSON.stringify(manifest, null, 2);
}

// Promote is allowed for any READY version, the live one included: a named
// alias may point at the version that is already live. The one guaranteed
// no-op (an alias already naming this version) is caught in the dialog.
function canPromoteRow(v: VersionResponse): boolean {
	return v.state === "READY";
}
function canRetireRow(v: VersionResponse): boolean {
	return !v.live && v.state !== "RETIRED";
}

const showPromoteDialog = ref(false);
const promoteTarget = ref<VersionResponse | null>(null);
const promoteAliasName = ref("live");
const promoting = ref(false);

const promoteAliasValid = computed(() =>
	DNS_LABEL_PATTERN.test(promoteAliasName.value.trim()),
);
const promoteWouldBeNoOp = computed(() => {
	const v = promoteTarget.value;
	if (!v) return false;
	const current = aliases.value.find((a) => a.alias === promoteAliasName.value.trim());
	return current?.versionId === v.id;
});

function openPromoteDialog(v: VersionResponse) {
	promoteTarget.value = v;
	promoteAliasName.value = "live";
	showPromoteDialog.value = true;
}

async function submitPromote() {
	const v = promoteTarget.value;
	if (!v || !promoteAliasValid.value || promoteWouldBeNoOp.value) return;
	const alias = promoteAliasName.value.trim();
	// Optimistic: the version this page shows the alias at (0: none yet), so
	// a promote made elsewhere since is a 412, not silently overwritten.
	const expectedVersion = aliases.value.find((a) => a.alias === alias)?.version ?? 0;
	promoting.value = true;
	try {
		await functionsApi.promote(props.address, v.version, alias, expectedVersion);
		toast.success("Success", `Version ${v.version} promoted to ${alias}`);
		showPromoteDialog.value = false;
		await Promise.all([loadVersions(props.address), loadAliases(props.address)]);
		emit("changed");
	} catch (e) {
		// Surfaced by the global error toast (e.g. SETTINGS_MISSING,
		// PUBLIC_ROUTE_TAKEN). A 412 means the aliases moved: show them as
		// they are now.
		if (e instanceof ApiError && e.status === 412) {
			await loadAliases(props.address);
		}
	} finally {
		promoting.value = false;
	}
}

function confirmRetire(v: VersionResponse) {
	confirm.require({
		message: `Retire version ${v.version}? It stops being loaded on any host.`,
		header: "Retire Version",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Retire",
		acceptClass: "p-button-warning",
		accept: () => retire(v),
	});
}

async function retire(v: VersionResponse) {
	try {
		await functionsApi.retireVersion(props.address, v.version);
		toast.success("Success", `Version ${v.version} retired`);
		await loadVersions(props.address);
		emit("changed");
	} catch {
		// surfaced by the global error toast (VERSION_IS_LIVE, VERSION_ALIASED, ...)
	}
}

function canDeleteAlias(a: AliasResponse): boolean {
	return a.alias !== "live";
}

function confirmDeleteAlias(a: AliasResponse) {
	confirm.require({
		message: `Remove alias "${a.alias}"? Anything still calling it stops working immediately.`,
		header: "Remove Alias",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Remove",
		acceptClass: "p-button-danger",
		accept: () => deleteAlias(a),
	});
}

async function deleteAlias(a: AliasResponse) {
	try {
		await functionsApi.deleteAlias(props.address, a.alias);
		toast.success("Success", `Alias ${a.alias} removed`);
		await loadAliases(props.address);
		emit("changed");
	} catch {
		// surfaced by the global error toast
	}
}

function onPublished(published: PublishResponse) {
	showPublishDialog.value = false;
	highlighted.value = published.version;
	void loadVersions(props.address);
	emit("changed");
}

function editorPath(): string {
	return `/functions/${encodeURIComponent(props.address)}/manifest`;
}

function openNewManifest() {
	void router.push(editorPath());
}

function openEditAsNewVersion(v: VersionResponse) {
	void router.push({ path: editorPath(), query: { fromVersion: String(v.version) } });
}

async function onImportManifestFile(event: Event) {
	const input = event.target as HTMLInputElement;
	const file = input.files?.[0];
	input.value = "";
	if (!file) return;
	try {
		setPendingManifestSeed(props.address, parseManifestText(await file.text()));
		void router.push({ path: editorPath(), query: { imported: "1" } });
	} catch {
		toast.error("Import failed", `${file.name} is not valid JSON`);
	}
}

function copyDigest(digest: string) {
	void navigator.clipboard.writeText(digest);
	toast.info("Copied", "Digest copied to clipboard");
}

function rowClass(data: VersionResponse) {
	return data.version === highlighted.value ? "highlighted-row" : undefined;
}
</script>

<template>
  <div class="versions-tab">
    <div v-if="canPublish" class="tab-toolbar">
      <Button
        label="New Manifest"
        icon="pi pi-file-plus"
        text
        data-testid="new-manifest-button"
        @click="openNewManifest"
      />
      <label class="import-label">
        <span class="import-label-text"><i class="pi pi-file-import" /> Import manifest</span>
        <input
          type="file"
          accept=".json,application/json"
          data-testid="import-manifest-input"
          @change="onImportManifestFile"
        />
      </label>
      <Button
        label="Publish Version"
        icon="pi pi-upload"
        data-testid="publish-version-button"
        @click="showPublishDialog = true"
      />
    </div>

    <ProgressSpinner v-if="loading" style="width: 24px; height: 24px" />
    <p v-else-if="versions.length === 0" class="text-muted text-sm">No versions published yet.</p>
    <DataTable
      v-else
      v-model:expandedRows="expandedRows"
      :value="versions"
      data-key="version"
      :row-class="rowClass"
      size="small"
      @row-expand="onRowExpand"
    >
      <Column expander style="width: 3rem" />
      <Column header="Version">
        <template #body="{ data }">
          <span>v{{ data.version }}</span>
          <Tag v-if="data.live" value="LIVE" severity="success" class="live-tag" />
        </template>
      </Column>
      <Column header="State">
        <template #body="{ data }">
          <Tag :value="data.state" :severity="versionStateSeverity(data.state)" />
        </template>
      </Column>
      <Column header="Digest">
        <template #body="{ data }">
          <span class="font-mono text-sm">{{ shortDigest(data.digest) }}</span>
          <Button
            icon="pi pi-copy"
            text
            size="small"
            v-tooltip="'Copy full digest'"
            @click="copyDigest(data.digest)"
          />
        </template>
      </Column>
      <Column header="Signer">
        <template #body="{ data }">
          <span v-if="data.signer" class="text-sm">{{ data.signer.issuer }} / {{ data.signer.subject }}</span>
          <span v-else class="text-muted">—</span>
        </template>
      </Column>
      <Column header="Published">
        <template #body="{ data }">
          <span class="text-sm">{{ formatDate(data.publishedAt) }}</span>
          <div class="text-muted text-xs">by {{ data.publishedBy }}</div>
        </template>
      </Column>
      <Column header="Actions">
        <template #body="{ data }">
          <div class="row-actions">
            <Button
              v-if="canPromote"
              label="Promote"
              size="small"
              text
              :disabled="!canPromoteRow(data)"
              data-testid="promote-button"
              @click="openPromoteDialog(data)"
            />
            <Button
              v-if="canPublish"
              label="Retire"
              size="small"
              text
              severity="danger"
              :disabled="!canRetireRow(data)"
              data-testid="retire-button"
              @click="confirmRetire(data)"
            />
            <Button
              v-if="canPublish"
              label="Edit as new version"
              size="small"
              text
              data-testid="edit-as-new-version-button"
              @click="openEditAsNewVersion(data)"
            />
          </div>
        </template>
      </Column>
      <template #expansion="{ data }">
        <div class="manifest-expansion">
          <div class="manifest-meta">
            <span><strong>Artifact ref:</strong> <code>{{ data.artifactRef }}</code></span>
            <span><strong>Pool:</strong> {{ data.pool }}</span>
            <span><strong>Warm:</strong> {{ data.warm ? "yes" : "no" }}</span>
            <span v-if="data.readyAt"><strong>Ready:</strong> {{ formatDate(data.readyAt) }}</span>
            <span v-if="data.retiredAt"><strong>Retired:</strong> {{ formatDate(data.retiredAt) }}</span>
          </div>
          <pre class="manifest-json">{{ prettyManifest(data.version) }}</pre>
        </div>
      </template>
    </DataTable>

    <h3 class="section-heading">Aliases</h3>
    <ProgressSpinner v-if="aliasesLoading" style="width: 24px; height: 24px" />
    <p v-else-if="aliases.length === 0" class="text-muted text-sm">No aliases yet.</p>
    <DataTable v-else :value="aliases" data-key="alias" size="small">
      <Column header="Alias">
        <template #body="{ data }">
          <span>{{ data.alias }}</span>
          <Tag v-if="data.alias === 'live'" value="LIVE" severity="success" class="live-tag" />
        </template>
      </Column>
      <Column header="Version">
        <template #body="{ data }">v{{ data.version }}</template>
      </Column>
      <Column header="Updated">
        <template #body="{ data }">
          <span class="text-sm">{{ formatDate(data.updatedAt) }} by {{ data.updatedBy }}</span>
        </template>
      </Column>
      <Column header="Actions">
        <template #body="{ data }">
          <Button
            v-if="canPromote"
            label="Remove"
            size="small"
            text
            severity="danger"
            :disabled="!canDeleteAlias(data)"
            v-tooltip="canDeleteAlias(data) ? undefined : 'live cannot be removed'"
            @click="confirmDeleteAlias(data)"
          />
        </template>
      </Column>
    </DataTable>

    <PublishVersionDialog
      v-if="showPublishDialog"
      :address="address"
      @close="showPublishDialog = false"
      @published="onPublished"
    />

    <Dialog
      v-model:visible="showPromoteDialog"
      header="Point Alias"
      modal
      :style="{ width: '28rem' }"
    >
      <p v-if="promoteTarget" class="promote-intro">
        Point an alias at version {{ promoteTarget.version }}.
        <template v-if="promoteAliasName.trim() === 'live'">
          This applies its manifest immediately: pools, subscriptions, schedules and public
          routes are created, updated or removed to match it.
        </template>
        <template v-else>
          Named aliases are HTTP-only: no wiring changes, no manifest applied.
        </template>
      </p>
      <div class="form-field">
        <label for="promoteAlias">Alias name</label>
        <InputText id="promoteAlias" v-model="promoteAliasName" class="full-width" autofocus />
        <small v-if="!promoteAliasValid" class="field-error">
          1-63 characters of a-z, 0-9 and '-', not starting or ending with '-'.
        </small>
        <small v-else-if="promoteWouldBeNoOp" class="field-error">
          Alias "{{ promoteAliasName.trim() }}" already points at version {{ promoteTarget?.version }}.
        </small>
      </div>
      <template #footer>
        <Button label="Cancel" text @click="showPromoteDialog = false" />
        <Button
          label="Promote"
          :disabled="!promoteAliasValid || promoting || promoteWouldBeNoOp"
          :loading="promoting"
          data-testid="promote-submit"
          @click="submitPromote"
        />
      </template>
    </Dialog>
  </div>
</template>

<style scoped>
.tab-toolbar {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
}

.import-label {
  position: relative;
  display: inline-flex;
  align-items: center;
  cursor: pointer;
  color: var(--p-primary-color);
  font-size: 0.875rem;
  font-weight: 500;
}

.import-label input {
  position: absolute;
  inset: 0;
  opacity: 0;
  cursor: pointer;
}

.import-label-text {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 0.4rem 0.625rem;
}

.section-heading {
  margin: 24px 0 12px;
  font-size: 1rem;
  font-weight: 600;
}

.font-mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.text-sm {
  font-size: 0.875rem;
}

.text-xs {
  font-size: 0.75rem;
}

.text-muted {
  color: var(--text-color-secondary);
}

.live-tag {
  margin-left: 6px;
}

.row-actions {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}

.manifest-expansion {
  padding: 12px 16px;
  background: var(--surface-ground);
}

.manifest-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 20px;
  font-size: 13px;
  margin-bottom: 10px;
}

.manifest-json {
  margin: 0;
  font-size: 12px;
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 360px;
  overflow: auto;
  background: #0f172a;
  color: #e2e8f0;
  padding: 12px;
  border-radius: 6px;
}

.promote-intro {
  font-size: 13px;
  color: var(--text-color-secondary);
  margin-top: 0;
}

.form-field label {
  display: block;
  margin-bottom: 6px;
  font-weight: 500;
}

.full-width {
  width: 100%;
}

.field-error {
  color: #dc2626;
  display: block;
  margin-top: 4px;
}

:deep(.highlighted-row) {
  background: #ecfdf5 !important;
}
</style>
