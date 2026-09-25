<script setup lang="ts">
// Publish a version: sha256 the artifact in the browser, upload it FIRST,
// then publish with the artifactRef the upload RETURNED (never a locally
// built `platform://…` string: only the platform knows the ref for its store).
// A failed upload never publishes. Error codes are shown verbatim.
import { computed, ref } from "vue";
import { toast } from "@/utils/errorBus";
import { ApiError } from "@/api/client";
import {
	functionsApi,
	type PublishManifestRequest,
	type PublishResponse,
} from "@/api/functions";
import { detailErrors } from "./format";

const props = defineProps<{
	address: string;
	/** Pre-fills the manifest text (the manifest editor's "Publish with this manifest"). */
	initialManifest?: PublishManifestRequest | null;
}>();

const emit = defineEmits<{
	close: [];
	published: [version: PublishResponse];
}>();

const visible = ref(true);

const artifactFile = ref<File | null>(null);
const manifestFile = ref<File | null>(null);
const manifestText = ref<string>(
	props.initialManifest ? JSON.stringify(props.initialManifest, null, 2) : "",
);
const bundleFile = ref<File | null>(null);

const submitting = ref(false);
const progressLabel = ref<string | null>(null);

const errorCode = ref<string | null>(null);
const errorMessage = ref<string | null>(null);
const fieldErrors = ref<Array<{ location?: string; message: string }>>([]);

const parsed = computed<{ manifest: PublishManifestRequest | null; error: string | null }>(() => {
	if (!manifestText.value.trim()) return { manifest: null, error: null };
	try {
		return { manifest: JSON.parse(manifestText.value) as PublishManifestRequest, error: null };
	} catch {
		return { manifest: null, error: "manifest.json is not valid JSON" };
	}
});

const canSubmit = computed(
	() => !!artifactFile.value && !!parsed.value.manifest && !submitting.value,
);

// The manifest's own `runtime` decides what the artifact input accepts. A
// `component` (or `wasm`, for a Rust host) artifact must be a WASI 0.2
// component: the platform refuses a core module for `component` at publish
// (ARTIFACT_RUNTIME_MISMATCH); a Rust host refuses one under `wasm` at load
// (WASM_CORE_MODULE_UNSUPPORTED).
const artifactRuntime = computed<"jvm" | "wasm">(() =>
	parsed.value.manifest?.runtime === "jvm" ? "jvm" : "wasm",
);
const artifactLabel = computed(() =>
	artifactRuntime.value === "wasm" ? "Wasm component" : "Jar file",
);
const artifactAccept = computed(() =>
	artifactRuntime.value === "wasm"
		? ".wasm,application/wasm,application/octet-stream"
		: ".jar,application/java-archive,application/octet-stream",
);

function onArtifactChange(event: Event) {
	artifactFile.value = (event.target as HTMLInputElement).files?.[0] ?? null;
}

async function onManifestChange(event: Event) {
	const file = (event.target as HTMLInputElement).files?.[0] ?? null;
	manifestFile.value = file;
	manifestText.value = file ? await file.text() : "";
}

function onBundleChange(event: Event) {
	bundleFile.value = (event.target as HTMLInputElement).files?.[0] ?? null;
}

async function sha256Hex(bytes: ArrayBuffer): Promise<string> {
	const digest = await crypto.subtle.digest("SHA-256", bytes);
	return Array.from(new Uint8Array(digest))
		.map((b) => b.toString(16).padStart(2, "0"))
		.join("");
}

function applyError(e: unknown) {
	if (e instanceof ApiError) {
		errorCode.value = e.code ?? null;
		errorMessage.value =
			e.code === "ARTIFACT_STORE_NOT_CONFIGURED"
				? "the platform has no artifact store configured"
				: e.message;
		fieldErrors.value = detailErrors(e.details);
	} else {
		errorCode.value = null;
		errorMessage.value = e instanceof Error ? e.message : "Publish failed";
		fieldErrors.value = [];
	}
}

async function onSubmit() {
	const manifest = parsed.value.manifest;
	const artifact = artifactFile.value;
	if (!artifact || !manifest || !canSubmit.value) return;

	errorCode.value = null;
	errorMessage.value = null;
	fieldErrors.value = [];
	submitting.value = true;
	try {
		progressLabel.value = "Hashing artifact…";
		const bytes = await artifact.arrayBuffer();
		const digest = `sha256:${await sha256Hex(bytes)}`;

		progressLabel.value = "Uploading artifact…";
		const upload = await functionsApi.uploadArtifact(props.address, digest, bytes, {
			suppressGlobalErrorToast: true,
		});

		progressLabel.value = "Publishing version…";
		const signatureBundle = bundleFile.value ? await bundleFile.value.text() : undefined;
		const published = await functionsApi.publishVersion(
			props.address,
			{ artifactRef: upload.artifactRef, digest, manifest, signatureBundle },
			{ suppressGlobalErrorToast: true },
		);

		toast.success("Success", `Version ${published.version} published`);
		emit("published", published);
		close();
	} catch (e) {
		applyError(e);
	} finally {
		submitting.value = false;
		progressLabel.value = null;
	}
}

function close() {
	visible.value = false;
	emit("close");
}
</script>

<template>
  <Dialog
    :visible="visible"
    header="Publish Version"
    modal
    :closable="!submitting"
    :style="{ width: '40rem' }"
    @update:visible="(v: boolean) => !v && close()"
  >
    <p class="dialog-subtitle font-mono">{{ address }}</p>

    <div class="form-field">
      <label>{{ artifactLabel }} <span class="required">*</span></label>
      <input
        type="file"
        :accept="artifactAccept"
        data-testid="publish-artifact-input"
        @change="onArtifactChange"
      />
      <small v-if="artifactFile" class="hint">{{ artifactFile.name }}</small>
    </div>

    <div class="form-field">
      <label>manifest.json <span class="required">*</span></label>
      <input
        type="file"
        accept=".json,application/json"
        data-testid="publish-manifest-input"
        @change="onManifestChange"
      />
      <small v-if="manifestFile" class="hint">{{ manifestFile.name }}</small>
      <small v-else-if="manifestText" class="hint" data-testid="publish-manifest-prefilled-hint">
        pre-filled from the manifest editor; choose a file to replace it
      </small>
      <small v-if="parsed.error" class="field-error">{{ parsed.error }}</small>
    </div>

    <div class="form-field">
      <label>Sigstore bundle (optional)</label>
      <input
        type="file"
        accept=".json,application/json"
        data-testid="publish-bundle-input"
        @change="onBundleChange"
      />
      <small v-if="bundleFile" class="hint">{{ bundleFile.name }}</small>
    </div>

    <div v-if="submitting" class="publish-progress" data-testid="publish-progress">
      <ProgressSpinner style="width: 20px; height: 20px" />
      <span>{{ progressLabel }}</span>
    </div>

    <Message v-if="errorMessage" severity="error" :closable="false" data-testid="publish-error">
      <div>
        <strong v-if="errorCode">{{ errorCode }}</strong>
        <span> {{ errorMessage }}</span>
      </div>
      <ul v-if="fieldErrors.length" class="field-errors">
        <li v-for="(fe, idx) in fieldErrors" :key="idx">
          <template v-if="fe.location">{{ fe.location }}: </template>{{ fe.message }}
        </li>
      </ul>
    </Message>

    <template #footer>
      <Button label="Cancel" severity="secondary" outlined :disabled="submitting" @click="close" />
      <Button
        label="Publish"
        icon="pi pi-upload"
        :loading="submitting"
        :disabled="!canSubmit"
        data-testid="publish-submit"
        @click="onSubmit"
      />
    </template>
  </Dialog>
</template>

<style scoped>
.dialog-subtitle {
  margin: 0 0 16px;
  font-size: 13px;
  color: var(--text-color-secondary);
}

.font-mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.form-field {
  margin-bottom: 16px;
}

.form-field > label {
  display: block;
  font-weight: 500;
  margin-bottom: 6px;
}

.required {
  color: #ef4444;
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

.publish-progress {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 0;
  font-size: 13px;
}

.field-errors {
  margin: 8px 0 0;
  padding-left: 18px;
  font-size: 13px;
}
</style>
