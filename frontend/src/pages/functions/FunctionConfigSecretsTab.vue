<script setup lang="ts">
// Config & secrets. Config values are plain and editable inline; a secret's
// value is never returned by the API and never stays in this component's
// state or DOM after a set. `declared`, `missing` and `declaredBy` come from
// the server: the union of the live manifest's keys and the newest
// non-retired version's (the candidate a promote checks).
import { computed, ref, watch } from "vue";
import { useConfirm } from "primevue/useconfirm";
import { toast } from "@/utils/errorBus";
import { ApiError } from "@/api/client";
import {
	functionsApi,
	type ConfigResponse,
	type SecretListResponse,
} from "@/api/functions";
import { useAuthStore } from "@/stores/auth";
import { userHasPermission } from "@/stores/permissions";
import { formatDate } from "./format";

const props = defineProps<{
	address: string;
	/** The live version, so "declared by v<n>" only flags keys live doesn't declare. */
	liveVersion?: number;
}>();

const authStore = useAuthStore();
const confirm = useConfirm();

const canManage = computed(() =>
	userHasPermission(authStore.user, "platform:function:secret:manage"),
);

const config = ref<ConfigResponse | null>(null);
const configLoading = ref(true);
const savingConfigKey = ref<string | null>(null);

const secrets = ref<SecretListResponse | null>(null);
const secretsLoading = ref(true);
/** The 503 reason when the platform has no app key: a disabled state, not an error toast. */
const secretsDisabledReason = ref<string | null>(null);
const savingSecretKey = ref<string | null>(null);

watch(
	() => props.address,
	async (addr) => {
		if (!addr) return;
		await Promise.all([loadConfig(addr), loadSecrets(addr)]);
	},
	{ immediate: true },
);

async function loadConfig(addr: string) {
	configLoading.value = true;
	try {
		config.value = await functionsApi.getConfig(addr);
	} catch {
		config.value = null;
	} finally {
		configLoading.value = false;
	}
}

async function loadSecrets(addr: string) {
	secretsLoading.value = true;
	secretsDisabledReason.value = null;
	try {
		secrets.value = await functionsApi.listSecrets(addr, undefined, {
			suppressGlobalErrorToast: true,
		});
	} catch (e) {
		secrets.value = null;
		if (e instanceof ApiError && e.status === 503) {
			secretsDisabledReason.value = e.message;
		} else {
			toast.error("Failed to load secrets", e instanceof Error ? e.message : undefined);
		}
	} finally {
		secretsLoading.value = false;
	}
}

/** key -> the versions whose manifest declares it. */
function declaredByVersions(
	entries: { version: number; keys: string[] }[] | undefined,
): Map<string, number[]> {
	const map = new Map<string, number[]>();
	for (const entry of entries ?? []) {
		for (const key of entry.keys) {
			const existing = map.get(key);
			if (existing) existing.push(entry.version);
			else map.set(key, [entry.version]);
		}
	}
	return map;
}

/** The newest declaring version to flag, or null when live declares the key (or nothing does). */
function nonLiveDeclaringVersion(declaredBy: number[]): number | null {
	if (declaredBy.length === 0) return null;
	if (props.liveVersion !== undefined && declaredBy.includes(props.liveVersion)) return null;
	return Math.max(...declaredBy);
}

interface ConfigRow {
	key: string;
	value: string | undefined;
	declaredBy: number[];
	declaredByTag: number | null;
}

const configRows = computed<ConfigRow[]>(() => {
	const declaredBy = declaredByVersions(config.value?.declaredBy);
	const values = config.value?.values ?? {};
	const keys = new Set<string>([...declaredBy.keys(), ...Object.keys(values)]);
	return [...keys].sort().map((key) => {
		const versions = declaredBy.get(key) ?? [];
		return {
			key,
			value: values[key],
			declaredBy: versions,
			declaredByTag: nonLiveDeclaringVersion(versions),
		};
	});
});

interface SecretRow {
	key: string;
	isSet: boolean;
	declaredBy: number[];
	declaredByTag: number | null;
	updatedAt?: string;
	updatedBy?: string;
}

const secretRows = computed<SecretRow[]>(() => {
	const declaredBy = declaredByVersions(secrets.value?.declaredBy);
	const byKey = new Map((secrets.value?.keys ?? []).map((k) => [k.key, k]));
	const keys = new Set<string>([...declaredBy.keys(), ...byKey.keys()]);
	return [...keys].sort().map((key) => {
		const entry = byKey.get(key);
		const versions = declaredBy.get(key) ?? [];
		return {
			key,
			isSet: !!entry,
			declaredBy: versions,
			declaredByTag: nonLiveDeclaringVersion(versions),
			updatedAt: entry?.updatedAt,
			updatedBy: entry?.updatedBy,
		};
	});
});

// A declared key with no value: promote refuses with SETTINGS_MISSING.
const missingCount = computed(
	() => (config.value?.missing?.length ?? 0) + (secrets.value?.missing?.length ?? 0),
);

// ── Config ──────────────────────────────────────────────────────────────────

const editingConfigKey = ref<string | null>(null);
const editingConfigValue = ref("");
const newConfigKey = ref("");
const newConfigValue = ref("");
const addingConfigKey = ref(false);

function startEditConfig(row: ConfigRow) {
	editingConfigKey.value = row.key;
	editingConfigValue.value = row.value ?? "";
}

function cancelEditConfig() {
	editingConfigKey.value = null;
	editingConfigValue.value = "";
}

/** `PUT …/config` replaces the whole map, so send every value plus this one. */
async function putConfigValue(key: string, value: string) {
	const values = { ...config.value?.values, [key]: value };
	config.value = await functionsApi.setConfig(props.address, { values });
}

async function saveConfig(key: string) {
	savingConfigKey.value = key;
	try {
		await putConfigValue(key, editingConfigValue.value);
		toast.success("Success", `${key} updated`);
		cancelEditConfig();
	} catch {
		// surfaced by the global error toast
	} finally {
		savingConfigKey.value = null;
	}
}

function confirmRemoveConfig(row: ConfigRow) {
	confirm.require({
		message: `Remove the config value "${row.key}"?`,
		header: "Remove Config Value",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Remove",
		acceptClass: "p-button-danger",
		accept: () => removeConfig(row.key),
	});
}

async function removeConfig(key: string) {
	const values = { ...config.value?.values };
	delete values[key];
	try {
		config.value = await functionsApi.setConfig(props.address, { values });
		toast.success("Success", `${key} removed`);
	} catch {
		// surfaced by the global error toast
	}
}

/** Sets a key no loaded manifest declares yet (one the operator knows is coming). */
async function addConfigKey() {
	const key = newConfigKey.value.trim();
	if (!key) return;
	addingConfigKey.value = true;
	try {
		await putConfigValue(key, newConfigValue.value);
		toast.success("Success", `${key} set`);
		newConfigKey.value = "";
		newConfigValue.value = "";
	} catch {
		// surfaced by the global error toast
	} finally {
		addingConfigKey.value = false;
	}
}

// ── Secrets ─────────────────────────────────────────────────────────────────

const editingSecretKey = ref<string | null>(null);
const editingSecretValue = ref("");
const newSecretKey = ref("");
const newSecretValue = ref("");
const addingSecretKey = ref(false);

function startSetSecret(row: SecretRow) {
	editingSecretKey.value = row.key;
	editingSecretValue.value = "";
}

function cancelSetSecret() {
	editingSecretKey.value = null;
	editingSecretValue.value = "";
}

async function putSecretValue(key: string, value: string) {
	await functionsApi.setSecret(props.address, key, { value }, { suppressGlobalErrorToast: true });
}

async function saveSecret(key: string) {
	savingSecretKey.value = key;
	try {
		await putSecretValue(key, editingSecretValue.value);
		toast.success("Success", `${key} set`);
	} catch (e) {
		toast.error("Failed to set secret", e instanceof ApiError ? e.message : "Request failed");
	} finally {
		// Clear the typed value whatever the outcome: it must never be
		// retrievable from this component again.
		cancelSetSecret();
		savingSecretKey.value = null;
		await loadSecrets(props.address);
	}
}

async function addSecretKey() {
	const key = newSecretKey.value.trim();
	const value = newSecretValue.value;
	if (!key || !value) return;
	addingSecretKey.value = true;
	try {
		await putSecretValue(key, value);
		toast.success("Success", `${key} set`);
	} catch (e) {
		toast.error("Failed to set secret", e instanceof ApiError ? e.message : "Request failed");
	} finally {
		newSecretKey.value = "";
		newSecretValue.value = "";
		addingSecretKey.value = false;
		await loadSecrets(props.address);
	}
}

function confirmDeleteSecret(row: SecretRow) {
	confirm.require({
		message: `Delete the secret "${row.key}"? A version that declares it can't be promoted until it is set again.`,
		header: "Delete Secret",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Delete",
		acceptClass: "p-button-danger",
		accept: () => deleteSecret(row.key),
	});
}

async function deleteSecret(key: string) {
	try {
		await functionsApi.deleteSecret(props.address, key, { suppressGlobalErrorToast: true });
		toast.success("Success", `${key} deleted`);
		await loadSecrets(props.address);
	} catch (e) {
		toast.error("Failed to delete secret", e instanceof ApiError ? e.message : "Request failed");
	}
}
</script>

<template>
  <div class="config-secrets-tab">
    <Message
      v-if="missingCount > 0"
      severity="warn"
      :closable="false"
      class="settings-banner"
      data-testid="settings-missing-banner"
    >
      {{ missingCount }} declared key{{ missingCount === 1 ? " has" : "s have" }} no value:
      promote refuses with <code>SETTINGS_MISSING</code> until every declared config and secret
      key has a value.
    </Message>

    <div class="section-card">
      <div class="card-header"><h3>Config</h3></div>
      <div class="card-content">
        <ProgressSpinner v-if="configLoading" style="width: 24px; height: 24px" />
        <template v-else>
          <p v-if="configRows.length === 0" class="text-muted text-sm">
            No config keys declared by the live manifest or the candidate version yet.
          </p>
          <table v-else class="kv-table">
            <thead>
              <tr>
                <th>Key</th>
                <th>Value</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="row in configRows" :key="row.key" data-testid="config-row">
                <td>
                  <code>{{ row.key }}</code>
                  <Tag
                    v-if="row.declaredBy.length === 0"
                    value="not declared"
                    severity="warn"
                    class="flag-tag"
                  />
                  <Tag
                    v-else-if="row.declaredByTag !== null"
                    :value="`declared by v${row.declaredByTag}`"
                    severity="info"
                    class="flag-tag"
                  />
                </td>
                <td>
                  <InputText
                    v-if="editingConfigKey === row.key"
                    v-model="editingConfigValue"
                    size="small"
                    class="value-input"
                  />
                  <span v-else-if="row.value !== undefined" class="font-mono text-sm">{{ row.value }}</span>
                  <span v-else class="unset">not set</span>
                </td>
                <td class="row-actions">
                  <template v-if="editingConfigKey === row.key">
                    <Button
                      icon="pi pi-check"
                      text
                      size="small"
                      :loading="savingConfigKey === row.key"
                      @click="saveConfig(row.key)"
                    />
                    <Button icon="pi pi-times" text size="small" @click="cancelEditConfig" />
                  </template>
                  <template v-else-if="canManage">
                    <Button icon="pi pi-pencil" text size="small" @click="startEditConfig(row)" />
                    <Button
                      v-if="row.value !== undefined"
                      icon="pi pi-trash"
                      text
                      size="small"
                      severity="danger"
                      @click="confirmRemoveConfig(row)"
                    />
                  </template>
                </td>
              </tr>
            </tbody>
          </table>

          <div v-if="canManage" class="add-key-row">
            <InputText v-model="newConfigKey" placeholder="Key" size="small" />
            <InputText v-model="newConfigValue" placeholder="Value" size="small" />
            <Button
              label="Add key"
              size="small"
              text
              :loading="addingConfigKey"
              :disabled="!newConfigKey.trim()"
              @click="addConfigKey"
            />
          </div>
        </template>
      </div>
    </div>

    <div class="section-card">
      <div class="card-header"><h3>Secrets</h3></div>
      <div class="card-content">
        <Message
          v-if="secretsDisabledReason"
          severity="secondary"
          :closable="false"
          data-testid="secrets-disabled"
        >
          Secrets management is unavailable: {{ secretsDisabledReason }}
        </Message>
        <ProgressSpinner v-else-if="secretsLoading" style="width: 24px; height: 24px" />
        <template v-else>
          <p v-if="secretRows.length === 0" class="text-muted text-sm">
            No secret keys declared by the live manifest or the candidate version yet.
          </p>
          <table v-else class="kv-table">
            <thead>
              <tr>
                <th>Key</th>
                <th>Status</th>
                <th>Updated</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="row in secretRows" :key="row.key" data-testid="secret-row">
                <td>
                  <code>{{ row.key }}</code>
                  <Tag
                    v-if="row.declaredBy.length === 0"
                    value="not declared"
                    severity="warn"
                    class="flag-tag"
                  />
                  <Tag
                    v-else-if="row.declaredByTag !== null"
                    :value="`declared by v${row.declaredByTag}`"
                    severity="info"
                    class="flag-tag"
                  />
                </td>
                <td>
                  <InputText
                    v-if="editingSecretKey === row.key"
                    v-model="editingSecretValue"
                    type="password"
                    size="small"
                    placeholder="New value"
                    autocomplete="off"
                    data-testid="secret-value-input"
                  />
                  <Tag
                    v-else
                    :value="row.isSet ? 'set' : 'not set'"
                    :severity="row.isSet ? 'success' : 'secondary'"
                  />
                </td>
                <td class="text-sm text-muted">
                  <template v-if="row.updatedAt">{{ formatDate(row.updatedAt) }} by {{ row.updatedBy }}</template>
                </td>
                <td class="row-actions">
                  <template v-if="editingSecretKey === row.key">
                    <Button
                      label="Save"
                      size="small"
                      :loading="savingSecretKey === row.key"
                      :disabled="!editingSecretValue"
                      data-testid="secret-save-button"
                      @click="saveSecret(row.key)"
                    />
                    <Button label="Cancel" text size="small" @click="cancelSetSecret" />
                  </template>
                  <template v-else-if="canManage">
                    <Button
                      :label="row.isSet ? 'Replace' : 'Set'"
                      text
                      size="small"
                      data-testid="secret-set-button"
                      @click="startSetSecret(row)"
                    />
                    <Button
                      v-if="row.isSet"
                      icon="pi pi-trash"
                      text
                      size="small"
                      severity="danger"
                      @click="confirmDeleteSecret(row)"
                    />
                  </template>
                </td>
              </tr>
            </tbody>
          </table>

          <div v-if="canManage" class="add-key-row">
            <InputText v-model="newSecretKey" placeholder="Key" size="small" />
            <InputText
              v-model="newSecretValue"
              type="password"
              placeholder="Value"
              size="small"
              autocomplete="off"
            />
            <Button
              label="Add key"
              size="small"
              text
              :loading="addingSecretKey"
              :disabled="!newSecretKey.trim() || !newSecretValue"
              @click="addSecretKey"
            />
          </div>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
.settings-banner {
  margin-bottom: 16px;
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

.kv-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.875rem;
}

.kv-table th {
  text-align: left;
  padding: 8px 12px;
  color: var(--text-color-secondary);
  font-weight: 600;
  border-bottom: 1px solid var(--surface-border);
}

.kv-table td {
  padding: 8px 12px;
  border-bottom: 1px solid var(--surface-border);
  vertical-align: middle;
}

.flag-tag {
  margin-left: 6px;
}

.value-input {
  width: 100%;
}

.unset {
  color: var(--text-color-secondary);
  font-style: italic;
}

.row-actions {
  white-space: nowrap;
  text-align: right;
}

.add-key-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px dashed var(--surface-border);
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
</style>
