<script setup lang="ts">
// One owner's function policy: the signer allow-list (each signer scoped to
// the runtimes it may publish for; the platform requires at least one) and
// the limit ceilings. `PUT` is a full replacement; an empty ceiling means the
// platform default. An owner with no stored policy reads as the platform
// defaults, with Create in place of Save.
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import { toast } from "@/utils/errorBus";
import {
	functionsApi,
	type PolicyResponse,
	type PolicySignerRequest,
} from "@/api/functions";
import { useReturnTo } from "@/composables/useReturnTo";
import { useClientOptions } from "@/composables/useClientOptions";
import { formatDate } from "@/pages/functions/format";

type Runtime = NonNullable<PolicySignerRequest["runtimes"]>[number];

interface SignerRow {
	issuer: string;
	subject: string;
	runtimes: Runtime[];
}

const route = useRoute();
const { returnTo } = useReturnTo();
const clientOptions = useClientOptions();

const owner = computed(() => route.params["owner"] as string);
const loading = ref(true);
const policy = ref<PolicyResponse | null>(null);
/** The platform owner's effective policy, shown beside each ceiling. */
const platformPolicy = ref<PolicyResponse | null>(null);
const saving = ref(false);

const signers = ref<SignerRow[]>([]);
const maxDurationMs = ref<number | null>(null);
const maxConcurrency = ref<number | null>(null);
const maxWasmMemoryMb = ref<number | null>(null);
const maxDbPoolSize = ref<number | null>(null);

const runtimeOptions: Array<{ label: string; value: Runtime }> = [
	{ label: "wasm", value: "wasm" },
	{ label: "jvm", value: "jvm" },
];

const ownerLabel = computed(() =>
	owner.value === "platform" ? "Platform" : clientOptions.getLabel(owner.value),
);

function formSnapshot(): string {
	return JSON.stringify({
		signers: signers.value,
		maxDurationMs: maxDurationMs.value,
		maxConcurrency: maxConcurrency.value,
		maxWasmMemoryMb: maxWasmMemoryMb.value,
		maxDbPoolSize: maxDbPoolSize.value,
	});
}
const cleanSnapshot = ref("");
const dirty = computed(() => formSnapshot() !== cleanSnapshot.value);

const signersValid = computed(() =>
	signers.value.every(
		(s) =>
			(s.issuer.trim() === "" && s.subject.trim() === "") ||
			(s.issuer.trim() !== "" && s.subject.trim() !== "" && s.runtimes.length > 0),
	),
);

onMounted(async () => {
	loading.value = true;
	try {
		if (owner.value !== "platform") void clientOptions.ensureLoaded().catch(() => {});
		policy.value = await functionsApi.getPolicy(owner.value);
		platformPolicy.value =
			owner.value === "platform"
				? null
				: await functionsApi.getPolicy("platform").catch(() => null);
		resetForm();
	} catch {
		policy.value = null;
	} finally {
		loading.value = false;
	}
});

function resetForm() {
	if (!policy.value) return;
	signers.value = policy.value.signers.map((s) => ({
		issuer: s.issuer,
		subject: s.subject,
		runtimes: [...s.runtimes],
	}));
	maxDurationMs.value = policy.value.ceilings.maxDurationMs;
	maxConcurrency.value = policy.value.ceilings.maxConcurrency;
	maxWasmMemoryMb.value = policy.value.ceilings.maxWasmMemoryMb;
	maxDbPoolSize.value = policy.value.ceilings.maxDbPoolSize;
	cleanSnapshot.value = formSnapshot();
}

function addSigner() {
	signers.value.push({ issuer: "", subject: "", runtimes: ["wasm"] });
}

function removeSigner(index: number) {
	signers.value.splice(index, 1);
}

async function save() {
	if (!signersValid.value) return;
	saving.value = true;
	const wasStored = policy.value?.stored ?? false;
	try {
		policy.value = await functionsApi.putPolicy(owner.value, {
			signers: signers.value
				.filter((s) => s.issuer.trim() && s.subject.trim())
				.map((s) => ({
					issuer: s.issuer.trim(),
					subject: s.subject.trim(),
					runtimes: s.runtimes,
				})),
			ceilings: {
				maxDurationMs: maxDurationMs.value ?? undefined,
				maxConcurrency: maxConcurrency.value ?? undefined,
				maxWasmMemoryMb: maxWasmMemoryMb.value ?? undefined,
				maxDbPoolSize: maxDbPoolSize.value ?? undefined,
			},
		});
		resetForm();
		toast.success("Success", wasStored ? "Policy saved" : "Policy created");
	} catch {
		// surfaced by the global error toast (SIGNER_INVALID, RUNTIME_INVALID, ...)
	} finally {
		saving.value = false;
	}
}

const platformHintLabel = computed(() =>
	platformPolicy.value?.stored ? "platform policy" : "platform default",
);
</script>

<template>
  <div class="page-container">
    <div v-if="loading" class="loading-container">
      <ProgressSpinner strokeWidth="3" />
    </div>

    <template v-else-if="policy">
      <header class="page-header">
        <div class="header-content">
          <Button
            icon="pi pi-arrow-left"
            text
            severity="secondary"
            v-tooltip="'Back to list'"
            @click="returnTo('/function-policies')"
          />
          <div class="header-text">
            <h1 class="page-title">{{ ownerLabel }}</h1>
            <p class="page-subtitle">
              Function policy · <code>{{ owner }}</code>
              <template v-if="policy.updatedAt"> · updated {{ formatDate(policy.updatedAt) }}</template>
            </p>
          </div>
          <Tag
            :value="policy.stored ? 'Custom Policy' : 'Platform Defaults'"
            :severity="policy.stored ? 'info' : 'secondary'"
          />
        </div>
      </header>

      <Message v-if="!policy.stored" severity="secondary" :closable="false" class="defaults-banner">
        This owner has no stored policy; the platform defaults apply. Create one below.
      </Message>

      <div class="section-card">
        <div class="card-header">
          <h3>Signers</h3>
          <Button icon="pi pi-plus" label="Add Signer" text @click="addSigner" />
        </div>
        <div class="card-content">
          <p v-if="signers.length === 0" class="text-muted text-sm">
            No permitted signer identities. When signatures are required, every publish for this
            owner is refused until one is added.
          </p>
          <div v-for="(signer, index) in signers" :key="index" class="signer-row" data-testid="signer-row">
            <InputText v-model="signer.issuer" placeholder="Issuer (e.g. https://token.actions.githubusercontent.com)" />
            <InputText v-model="signer.subject" placeholder="Subject (an email or a workflow URI)" />
            <MultiSelect
              v-model="signer.runtimes"
              :options="runtimeOptions"
              optionLabel="label"
              optionValue="value"
              placeholder="Runtimes"
              :invalid="signer.runtimes.length === 0"
            />
            <Button icon="pi pi-trash" text severity="danger" v-tooltip="'Remove'" @click="removeSigner(index)" />
          </div>
          <small v-if="!signersValid" class="field-error">
            Each signer needs an issuer, a subject and at least one runtime.
          </small>
        </div>
      </div>

      <div class="section-card">
        <div class="card-header"><h3>Limit Ceilings</h3></div>
        <div class="card-content">
          <p class="text-muted text-sm ceilings-intro">
            The most a manifest of this owner may ask for. Leave a field empty for the platform default.
          </p>
          <div class="form-grid">
            <div class="form-field">
              <label for="ceil-duration">Max Duration (ms)</label>
              <InputNumber v-model="maxDurationMs" inputId="ceil-duration" :min="1" class="full-width" />
              <small v-if="platformPolicy" class="hint">
                {{ platformHintLabel }}: {{ platformPolicy.ceilings.maxDurationMs }} ms
              </small>
            </div>
            <div class="form-field">
              <label for="ceil-concurrency">Max Concurrency</label>
              <InputNumber v-model="maxConcurrency" inputId="ceil-concurrency" :min="1" class="full-width" />
              <small v-if="platformPolicy" class="hint">
                {{ platformHintLabel }}: {{ platformPolicy.ceilings.maxConcurrency }}
              </small>
            </div>
            <div class="form-field">
              <label for="ceil-memory">Max Wasm Memory (MB)</label>
              <InputNumber v-model="maxWasmMemoryMb" inputId="ceil-memory" :min="1" class="full-width" />
              <small v-if="platformPolicy" class="hint">
                {{ platformHintLabel }}: {{ platformPolicy.ceilings.maxWasmMemoryMb }} MB
              </small>
            </div>
            <div class="form-field">
              <label for="ceil-db">Max DB Pool Size</label>
              <InputNumber v-model="maxDbPoolSize" inputId="ceil-db" :min="1" class="full-width" />
              <small v-if="platformPolicy" class="hint">
                {{ platformHintLabel }}: {{ platformPolicy.ceilings.maxDbPoolSize }}
              </small>
            </div>
          </div>
        </div>
      </div>

      <div class="form-actions">
        <Button v-if="dirty" label="Discard" severity="secondary" outlined @click="resetForm" />
        <Button
          :label="policy.stored ? 'Save' : 'Create'"
          :disabled="(!dirty && policy.stored) || !signersValid"
          :loading="saving"
          data-testid="policy-save"
          @click="save"
        />
      </div>
    </template>

    <Message v-else severity="error">Could not load this owner's policy</Message>
  </div>
</template>

<style scoped>
.page-container {
  max-width: 1000px;
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

.header-text {
  flex: 1;
}

.defaults-banner {
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

.signer-row {
  display: grid;
  grid-template-columns: 1fr 1fr 12rem auto;
  gap: 8px;
  align-items: center;
  margin-bottom: 8px;
}

.ceilings-intro {
  margin: 0 0 12px;
}

.form-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 16px;
}

.form-field label {
  display: block;
  margin-bottom: 6px;
  font-weight: 500;
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

.field-error {
  display: block;
  margin-top: 4px;
  color: #dc2626;
}

.text-sm {
  font-size: 0.875rem;
}

.text-muted {
  color: var(--text-color-secondary);
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: 12px;
}

@media (max-width: 760px) {
  .signer-row {
    grid-template-columns: 1fr;
  }
}
</style>
