<script setup lang="ts">
// One owner's function policy: the signer allow-list (each signer scoped to
// the runtimes it may publish for; the platform requires at least one) and
// the limit ceilings. `PUT` is a full replacement; an empty ceiling means the
// platform default. An owner with no stored policy reads as the platform
// defaults, with Create in place of Save.
import { computed, ref, watch } from "vue";
import { toast } from "@/utils/errorBus";
import {
	functionsApi,
	type PolicyResponse,
	type PolicySignerRequest,
} from "@/api/functions";
import { useClientOptions } from "@/composables/useClientOptions";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";
import { useDirtyForm } from "@/composables/useDirtyForm";
import { formatDate } from "@/pages/functions/format";

type Runtime = NonNullable<PolicySignerRequest["runtimes"]>[number];

interface SignerRow {
	issuer: string;
	subject: string;
	runtimes: Runtime[];
}

const emit = defineEmits<{
	changed: [];
}>();

const clientOptions = useClientOptions();

const signers = ref<SignerRow[]>([]);
const maxDurationMs = ref<number | null>(null);
const maxConcurrency = ref<number | null>(null);
const maxWasmMemoryMb = ref<number | null>(null);
const maxDbPoolSize = ref<number | null>(null);

const { dirty, markClean, reset: resetDirty } = useDirtyForm(() => ({
	signers: signers.value,
	maxDurationMs: maxDurationMs.value,
	maxConcurrency: maxConcurrency.value,
	maxWasmMemoryMb: maxWasmMemoryMb.value,
	maxDbPoolSize: maxDbPoolSize.value,
}));

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { id: owner, goToList } = useDrawerRoute({
	listPath: "/function-policies",
	paramKey: "owner",
	dirty,
});

const loading = ref(true);
const policy = ref<PolicyResponse | null>(null);
/** The platform owner's effective policy, shown beside each ceiling. */
const platformPolicy = ref<PolicyResponse | null>(null);
const saving = ref(false);

const runtimeOptions: Array<{ label: string; value: Runtime }> = [
	{ label: "wasm", value: "wasm" },
	{ label: "jvm", value: "jvm" },
];

const ownerLabel = computed(() =>
	owner.value === "platform" ? "Platform" : clientOptions.getLabel(owner.value ?? ""),
);

const signersValid = computed(() =>
	signers.value.every(
		(s) =>
			(s.issuer.trim() === "" && s.subject.trim() === "") ||
			(s.issuer.trim() !== "" && s.subject.trim() !== "" && s.runtimes.length > 0),
	),
);

// Reactive param: the drawer instance is reused when switching between rows.
watch(
	owner,
	async (value) => {
		if (!value) return;
		resetDirty();
		await load(value);
	},
	{ immediate: true },
);

async function load(ownerId: string) {
	loading.value = true;
	try {
		if (ownerId !== "platform") void clientOptions.ensureLoaded().catch(() => {});
		policy.value = await functionsApi.getPolicy(ownerId);
		platformPolicy.value =
			ownerId === "platform"
				? null
				: await functionsApi.getPolicy("platform").catch(() => null);
		resetForm();
	} catch {
		policy.value = null;
	} finally {
		loading.value = false;
	}
}

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
	markClean();
}

function addSigner() {
	signers.value.push({ issuer: "", subject: "", runtimes: ["wasm"] });
}

function removeSigner(index: number) {
	signers.value.splice(index, 1);
}

async function save() {
	if (!signersValid.value || !owner.value) return;
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
		emit("changed");
	} catch {
		// surfaced by the global error toast (SIGNER_INVALID, RUNTIME_INVALID, ...)
	} finally {
		saving.value = false;
	}
}

const platformHintLabel = computed(() =>
	platformPolicy.value?.stored ? "platform policy" : "platform default",
);

const subtitle = computed(() => {
	if (!owner.value) return "Function policy";
	const updated = policy.value?.updatedAt ? ` · updated ${formatDate(policy.value.updatedAt)}` : "";
	return `Function policy · ${owner.value}${updated}`;
});
</script>

<template>
  <EntityDrawer
    ref="drawer"
    :title="ownerLabel || 'Function Policy'"
    :subtitle="subtitle"
    size="wide"
    :loading="loading"
    :error="!loading && !policy ? 'Could not load this owner\'s policy' : null"
    :dirty="dirty"
    @close="goToList()"
  >
    <template v-if="policy" #header-extra>
      <Tag
        :value="policy.stored ? 'Custom Policy' : 'Platform Defaults'"
        :severity="policy.stored ? 'info' : 'secondary'"
      />
    </template>

    <template v-if="policy">
      <Message v-if="!policy.stored" severity="secondary" :closable="false" class="defaults-banner">
        This owner has no stored policy; the platform defaults apply. Create one below.
      </Message>

      <FcFormSection title="Signers" flat>
        <template #actions>
          <Button icon="pi pi-plus" label="Add Signer" text @click="addSigner" />
        </template>
        <p v-if="signers.length === 0" class="text-muted text-sm">
          No permitted signer identities. When signatures are required, every publish for this
          owner is refused until one is added.
        </p>
        <div
          v-for="(signer, index) in signers"
          :key="index"
          class="signer-row"
          data-testid="signer-row"
        >
          <InputText
            v-model="signer.issuer"
            placeholder="Issuer (e.g. https://token.actions.githubusercontent.com)"
          />
          <InputText v-model="signer.subject" placeholder="Subject (an email or a workflow URI)" />
          <MultiSelect
            v-model="signer.runtimes"
            :options="runtimeOptions"
            optionLabel="label"
            optionValue="value"
            placeholder="Runtimes"
            :invalid="signer.runtimes.length === 0"
          />
          <Button
            icon="pi pi-trash"
            text
            severity="danger"
            v-tooltip="'Remove'"
            @click="removeSigner(index)"
          />
        </div>
        <small v-if="!signersValid" class="fc-field-error">
          Each signer needs an issuer, a subject and at least one runtime.
        </small>
      </FcFormSection>

      <FcFormSection
        title="Limit Ceilings"
        description="The most a manifest of this owner may ask for. Leave a field empty for the platform default."
        flat
      >
        <div class="fc-form-grid">
          <FcFormField label="Max Duration (ms)">
            <template #default="{ id: fieldId }">
              <InputNumber v-model="maxDurationMs" :inputId="fieldId" :min="1" />
            </template>
            <template v-if="platformPolicy" #help>
              {{ platformHintLabel }}: {{ platformPolicy.ceilings.maxDurationMs }} ms
            </template>
          </FcFormField>
          <FcFormField label="Max Concurrency">
            <template #default="{ id: fieldId }">
              <InputNumber v-model="maxConcurrency" :inputId="fieldId" :min="1" />
            </template>
            <template v-if="platformPolicy" #help>
              {{ platformHintLabel }}: {{ platformPolicy.ceilings.maxConcurrency }}
            </template>
          </FcFormField>
          <FcFormField label="Max Wasm Memory (MB)">
            <template #default="{ id: fieldId }">
              <InputNumber v-model="maxWasmMemoryMb" :inputId="fieldId" :min="1" />
            </template>
            <template v-if="platformPolicy" #help>
              {{ platformHintLabel }}: {{ platformPolicy.ceilings.maxWasmMemoryMb }} MB
            </template>
          </FcFormField>
          <FcFormField label="Max DB Pool Size">
            <template #default="{ id: fieldId }">
              <InputNumber v-model="maxDbPoolSize" :inputId="fieldId" :min="1" />
            </template>
            <template v-if="platformPolicy" #help>
              {{ platformHintLabel }}: {{ platformPolicy.ceilings.maxDbPoolSize }}
            </template>
          </FcFormField>
        </div>
      </FcFormSection>
    </template>

    <template v-if="policy" #footer>
      <FcFormActions :bordered="false">
        <Button v-if="dirty" label="Discard" severity="secondary" outlined @click="resetForm" />
        <Button
          :label="policy.stored ? 'Save' : 'Create'"
          :disabled="(!dirty && policy.stored) || !signersValid"
          :loading="saving"
          data-testid="policy-save"
          @click="save"
        />
      </FcFormActions>
    </template>
  </EntityDrawer>
</template>

<style scoped>
.defaults-banner {
  margin-bottom: 16px;
}

.signer-row {
  display: grid;
  grid-template-columns: 1fr 1fr 10rem auto;
  gap: 8px;
  align-items: center;
  margin-bottom: 8px;
}

.text-sm {
  font-size: 0.875rem;
}

.text-muted {
  color: var(--text-color-secondary);
}

@media (max-width: 760px) {
  .signer-row {
    grid-template-columns: 1fr;
  }
}
</style>
