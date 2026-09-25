<script setup lang="ts">
// The manifest editor: two synced views of one model (a plain
// PublishManifestRequest). A form covers every field of the manifest schema;
// a JSON textarea shows the same document. A form edit re-serialises the
// JSON; a valid JSON edit re-parses into the form; an invalid one leaves the
// form on the last valid model (disabled, with a warning) until it parses.
// The server is the only validator (`POST …/manifest/check`).
//
// Started three ways: `?fromVersion=<n>` (a version's stored manifest),
// `?imported=1` (a file chosen on the Versions tab), or neither (the new
// manifest template for the function's runtime).
import { computed, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { toast } from "@/utils/errorBus";
import { ApiError } from "@/api/client";
import {
	functionsApi,
	type CheckManifestResponse,
	type FunctionResponse,
	type ManifestErrorResponse,
	type PublishResponse,
} from "@/api/functions";
import {
	cloneManifestModel,
	exportManifest,
	manifestToPrettyJson,
	newManifestModel,
	parseManifestText,
	type ManifestModel,
} from "./manifestModel";
import { renderPlanLines } from "./manifestPlanText";
import { takePendingManifestSeed } from "./manifestSeed";
import PublishVersionDialog from "./PublishVersionDialog.vue";

const route = useRoute();
const router = useRouter();

const address = computed(() => route.params["address"] as string);
const fn = ref<FunctionResponse | null>(null);
const loading = ref(true);
const loadError = ref<string | null>(null);
const seedLabel = ref("New manifest");

const model = ref<ManifestModel>(newManifestModel());
const view = ref<"form" | "json">("form");
const jsonDraft = ref(manifestToPrettyJson(model.value));
const jsonParseError = ref<string | null>(null);

// Form -> JSON: any model change re-serialises the JSON view.
watch(
	model,
	() => {
		jsonDraft.value = manifestToPrettyJson(model.value);
	},
	{ deep: true },
);

function onJsonInput(text: string | undefined) {
	jsonDraft.value = text ?? "";
	try {
		const parsed = parseManifestText(jsonDraft.value);
		jsonParseError.value = null;
		model.value = parsed;
	} catch (e) {
		// The model is left alone: an invalid document never reaches the form,
		// Validate, Publish or Export half-applied.
		jsonParseError.value = e instanceof Error ? e.message : "invalid JSON";
	}
}

const formReadOnly = computed(() => jsonParseError.value !== null);

onMounted(async () => {
	loading.value = true;
	try {
		fn.value = await functionsApi.get(address.value);
		const fromVersion = Number(route.query["fromVersion"]);
		const imported = route.query["imported"] === "1" ? takePendingManifestSeed(address.value) : null;
		if (imported) {
			model.value = cloneManifestModel(imported);
			seedLabel.value = "Imported manifest";
		} else if (Number.isInteger(fromVersion) && fromVersion > 0) {
			const version = await functionsApi.getVersion(address.value, fromVersion);
			model.value = version.manifest
				? cloneManifestModel(version.manifest as ManifestModel)
				: newManifestModel(fn.value.runtime);
			seedLabel.value = `Edit v${fromVersion} as a new version`;
		} else {
			model.value = newManifestModel(fn.value.runtime);
		}
	} catch {
		loadError.value = "Function or version not found";
	} finally {
		loading.value = false;
	}
});

function backToVersions(highlight?: number) {
	const query: Record<string, string> = { tab: "versions" };
	if (highlight !== undefined) query["highlight"] = String(highlight);
	void router.push({ path: `/functions/${encodeURIComponent(address.value)}`, query });
}

// ── Validate ────────────────────────────────────────────────────────────────

const checkAlias = ref("live");
const validating = ref(false);
const checkResult = ref<CheckManifestResponse | null>(null);
const checkTransportError = ref<string | null>(null);

const planLines = computed(() =>
	checkResult.value?.plan ? renderPlanLines(checkResult.value.plan) : [],
);

// An error's `details.pointer` (RFC 6901) attaches it to the field it names
// (`/endpoints/1/auth` under endpoint 1's Auth), else its row, else its
// section (a whole-array pointer, or no pointer at all by code prefix). The
// flat list under Validate always shows every error regardless.
function errorPointer(e: ManifestErrorResponse): string | undefined {
	const pointer = e.details?.pointer;
	return typeof pointer === "string" ? pointer : undefined;
}

function fieldError(topKey: string, i: number, field: string): string | undefined {
	const target = `/${topKey}/${i}/${field}`;
	return (checkResult.value?.errors ?? []).find((e) => errorPointer(e) === target)?.message;
}

function topFieldError(field: string): string | undefined {
	return (checkResult.value?.errors ?? []).find((e) => errorPointer(e) === `/${field}`)?.message;
}

function rowErrors(topKey: string, i: number): ManifestErrorResponse[] {
	const target = `/${topKey}/${i}`;
	return (checkResult.value?.errors ?? []).filter((e) => errorPointer(e) === target);
}

function sectionErrors(topKey: string, codePrefix: string): ManifestErrorResponse[] {
	return (checkResult.value?.errors ?? []).filter((e) => {
		const pointer = errorPointer(e);
		if (pointer === undefined) return e.code.startsWith(codePrefix);
		return pointer === `/${topKey}`;
	});
}

const endpointErrors = computed(() => sectionErrors("endpoints", "ENDPOINT_"));
const subscriptionErrors = computed(() => sectionErrors("subscriptions", "SUBSCRIPTION_"));
const scheduleErrors = computed(() => sectionErrors("schedules", "SCHEDULE_"));
const publicRouteErrors = computed(() => sectionErrors("public", "PUBLIC_ROUTE_"));
const dbErrors = computed(() => sectionErrors("db", "DB_"));

async function validate() {
	validating.value = true;
	checkTransportError.value = null;
	checkResult.value = null;
	try {
		checkResult.value = await functionsApi.checkManifest(address.value, {
			manifest: model.value,
			alias: checkAlias.value.trim() || undefined,
		});
	} catch (e) {
		checkTransportError.value =
			e instanceof ApiError || e instanceof Error ? e.message : "Validate failed";
	} finally {
		validating.value = false;
	}
}

// ── List editing ────────────────────────────────────────────────────────────

type Endpoint = NonNullable<ManifestModel["endpoints"]>[number];
type Subscription = NonNullable<ManifestModel["subscriptions"]>[number];
type Schedule = NonNullable<ManifestModel["schedules"]>[number];

function addEndpoint() {
	(model.value.endpoints ??= []).push({ path: "", auth: "platform" });
}
function removeEndpoint(i: number) {
	model.value.endpoints?.splice(i, 1);
}
function toggleCors(ep: Endpoint) {
	if (ep.cors) delete ep.cors;
	else ep.cors = { origins: [], methods: [], headers: [], allowCredentials: false };
}

function addSubscription() {
	(model.value.subscriptions ??= []).push({ eventType: "", path: "" });
}
function removeSubscription(i: number) {
	model.value.subscriptions?.splice(i, 1);
}

function addSchedule() {
	(model.value.schedules ??= []).push({ cron: "", path: "" });
}
function removeSchedule(i: number) {
	model.value.schedules?.splice(i, 1);
}
function schedulePayloadText(s: Schedule): string {
	return s.payload === undefined ? "" : JSON.stringify(s.payload);
}
const schedulePayloadErrors = ref<Record<number, string | undefined>>({});
function setSchedulePayload(s: Schedule, index: number, text: string | undefined) {
	if (!text || !text.trim()) {
		delete s.payload;
		schedulePayloadErrors.value[index] = undefined;
		return;
	}
	try {
		s.payload = JSON.parse(text);
		schedulePayloadErrors.value[index] = undefined;
	} catch {
		schedulePayloadErrors.value[index] = "payload is not valid JSON";
	}
}

function addPublicRoute() {
	(model.value.public ??= []).push({ hostname: "" });
}
function removePublicRoute(i: number) {
	model.value.public?.splice(i, 1);
}

function addDbRef() {
	(model.value.db ??= []).push({ name: "", secretRef: "" });
}
function removeDbRef(i: number) {
	model.value.db?.splice(i, 1);
}

function removeLimits() {
	delete model.value.limits;
}

function joinList(arr?: string[]): string {
	return (arr ?? []).join(", ");
}
function parseList(text: string | undefined): string[] | undefined {
	const items = (text ?? "")
		.split(",")
		.map((s) => s.trim())
		.filter(Boolean);
	return items.length ? items : undefined;
}

// Each option list is the key set of a Record over the generated union, so
// the compiler refuses a missing or an extra value.
const optionsOf = <T extends string>(all: Record<T, true>): T[] => Object.keys(all) as T[];
const runtimeOptions = optionsOf<ManifestModel["runtime"]>({
	component: true,
	wasm: true,
	jvm: true,
});
const authOptions = optionsOf<Endpoint["auth"]>({ webhook: true, platform: true, none: true });
const methodOptions = optionsOf<NonNullable<Endpoint["methods"]>[number]>({
	GET: true,
	HEAD: true,
	POST: true,
	PUT: true,
	PATCH: true,
	DELETE: true,
	OPTIONS: true,
});
const modeOptions = optionsOf<NonNullable<Subscription["mode"]>>({
	IMMEDIATE: true,
	NEXT_ON_ERROR: true,
	BLOCK_ON_ERROR: true,
});

// ── Import / export / publish ───────────────────────────────────────────────

function exportFile() {
	const text = JSON.stringify(exportManifest(model.value), null, 2);
	const blob = new Blob([text], { type: "application/json" });
	const url = URL.createObjectURL(blob);
	const a = document.createElement("a");
	a.href = url;
	a.download = "manifest.json";
	a.click();
	URL.revokeObjectURL(url);
}

async function onImportFileChange(event: Event) {
	const input = event.target as HTMLInputElement;
	const file = input.files?.[0];
	input.value = "";
	if (!file) return;
	try {
		model.value = parseManifestText(await file.text());
		jsonParseError.value = null;
		toast.success("Imported", `Loaded ${file.name}`);
	} catch {
		toast.error("Import failed", `${file.name} is not valid JSON`);
	}
}

const showPublishDialog = ref(false);

function onPublished(published: PublishResponse) {
	showPublishDialog.value = false;
	backToVersions(published.version);
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div class="header-content">
        <Button
          icon="pi pi-arrow-left"
          text
          severity="secondary"
          v-tooltip="'Back to versions'"
          @click="backToVersions()"
        />
        <div class="header-text">
          <h1 class="page-title">Manifest Editor</h1>
          <p class="page-subtitle">
            <code>{{ address }}</code> · {{ seedLabel }}
          </p>
        </div>
      </div>
      <div class="header-actions">
        <Button
          label="Publish with this manifest"
          icon="pi pi-upload"
          :disabled="formReadOnly || !fn"
          data-testid="manifest-publish-button"
          @click="showPublishDialog = true"
        />
      </div>
    </header>

    <div v-if="loading" class="loading-container">
      <ProgressSpinner strokeWidth="3" />
    </div>

    <Message v-else-if="loadError" severity="error">{{ loadError }}</Message>

    <template v-else>
      <div class="fc-card">
        <div class="editor-toolbar">
          <SelectButton
            v-model="view"
            :options="[
              { label: 'Form', value: 'form' },
              { label: 'JSON', value: 'json' },
            ]"
            optionLabel="label"
            optionValue="value"
            :allowEmpty="false"
            data-testid="manifest-view-toggle"
          />
          <div class="editor-toolbar-actions">
            <label class="import-label">
              <span class="import-label-text"><i class="pi pi-file-import" /> Import file</span>
              <input
                type="file"
                accept=".json,application/json"
                data-testid="manifest-import-input"
                @change="onImportFileChange"
              />
            </label>
            <Button
              label="Export"
              icon="pi pi-download"
              text
              data-testid="manifest-export-button"
              @click="exportFile"
            />
          </div>
        </div>

        <Message v-if="formReadOnly" severity="warn" :closable="false" data-testid="manifest-json-error">
          The JSON is not valid; the form shows the last valid version until it is fixed:
          {{ jsonParseError }}
        </Message>

        <div v-show="view === 'json'">
          <Textarea
            :modelValue="jsonDraft"
            rows="28"
            class="manifest-json-textarea"
            spellcheck="false"
            data-testid="manifest-json-textarea"
            @update:modelValue="onJsonInput"
          />
        </div>

        <div v-show="view === 'form'" :class="{ 'form-disabled': formReadOnly }">
          <fieldset :disabled="formReadOnly" class="form-fieldset">
            <section class="editor-section">
              <h3>Runtime</h3>
              <div class="form-grid">
                <div class="form-field">
                  <label>Runtime <span class="required">*</span></label>
                  <Select v-model="model.runtime" :options="runtimeOptions" class="full-width" />
                  <small v-if="topFieldError('runtime')" class="field-error">{{ topFieldError("runtime") }}</small>
                </div>
                <div class="form-field">
                  <label>
                    Entrypoint <span v-if="model.runtime !== 'component'" class="required">*</span>
                  </label>
                  <InputText v-model="model.entrypoint" class="full-width" data-testid="manifest-entrypoint-input" />
                  <small v-if="topFieldError('entrypoint')" class="field-error">{{ topFieldError("entrypoint") }}</small>
                  <small v-else-if="model.runtime === 'component'" class="hint">
                    Optional: <code>wasi:http/incoming-handler</code> when blank.
                  </small>
                  <small v-else-if="model.runtime === 'wasm'" class="hint">
                    A component's <code>wasi_http_incoming_handler</code> (or
                    <code>wasi:http/incoming-handler</code>).
                  </small>
                </div>
                <div class="form-field">
                  <label>Pool</label>
                  <InputText v-model="model.pool" class="full-width" />
                  <small v-if="topFieldError('pool')" class="field-error">{{ topFieldError("pool") }}</small>
                  <small v-else class="hint">Defaults to "default" when absent.</small>
                </div>
                <div class="form-field checkbox-field">
                  <Checkbox v-model="model.warm" binary inputId="manifest-warm" />
                  <label for="manifest-warm">Warm (loaded eagerly)</label>
                </div>
              </div>
            </section>

            <section class="editor-section">
              <h3>Limits</h3>
              <template v-if="model.limits">
                <div class="form-grid">
                  <div class="form-field">
                    <label>Max duration (ms)</label>
                    <InputNumber
                      :modelValue="model.limits.maxDurationMs ?? null"
                      class="full-width"
                      @update:modelValue="(v: number | null) => (model.limits!.maxDurationMs = v ?? undefined)"
                    />
                  </div>
                  <div class="form-field">
                    <label>Max concurrency</label>
                    <InputNumber
                      :modelValue="model.limits.maxConcurrency ?? null"
                      class="full-width"
                      @update:modelValue="(v: number | null) => (model.limits!.maxConcurrency = v ?? undefined)"
                    />
                  </div>
                  <div v-if="model.runtime !== 'jvm'" class="form-field">
                    <label>Wasm memory (MB)</label>
                    <InputNumber
                      :modelValue="model.limits.wasmMemoryMb ?? null"
                      class="full-width"
                      @update:modelValue="(v: number | null) => (model.limits!.wasmMemoryMb = v ?? undefined)"
                    />
                  </div>
                </div>
                <Button label="Remove limits override" text size="small" @click="removeLimits" />
              </template>
              <Button v-else label="Add limits override" text size="small" @click="model.limits = {}" />
            </section>

            <section class="editor-section">
              <h3>Endpoints</h3>
              <Message
                v-for="(err, idx) in endpointErrors"
                :key="`s${idx}`"
                severity="error"
                :closable="false"
                data-testid="endpoints-section-error"
              >
                <strong>{{ err.code }}</strong> {{ err.message }}
              </Message>
              <div v-for="(ep, i) in model.endpoints ?? []" :key="i" class="list-row" data-testid="endpoint-row">
                <Message v-for="(err, idx) in rowErrors('endpoints', i)" :key="idx" severity="error" :closable="false">
                  <strong>{{ err.code }}</strong> {{ err.message }}
                </Message>
                <div class="form-grid">
                  <div class="form-field">
                    <label>Path <span class="required">*</span></label>
                    <InputText v-model="ep.path" class="full-width" />
                    <small v-if="fieldError('endpoints', i, 'path')" class="field-error">{{ fieldError("endpoints", i, "path") }}</small>
                  </div>
                  <div class="form-field">
                    <label>Auth <span class="required">*</span></label>
                    <Select v-model="ep.auth" :options="authOptions" class="full-width" />
                    <small
                      v-if="fieldError('endpoints', i, 'auth')"
                      class="field-error"
                      data-testid="endpoint-auth-error"
                    >{{ fieldError("endpoints", i, "auth") }}</small>
                  </div>
                  <div class="form-field">
                    <label>Methods</label>
                    <MultiSelect
                      :modelValue="ep.methods ?? []"
                      :options="methodOptions"
                      class="full-width"
                      @update:modelValue="(v: string[]) => (ep.methods = v.length ? (v as Endpoint['methods']) : undefined)"
                    />
                    <small v-if="fieldError('endpoints', i, 'methods')" class="field-error">{{ fieldError("endpoints", i, "methods") }}</small>
                    <small v-else class="hint">Empty means every method.</small>
                  </div>
                  <div class="form-field">
                    <label>Max body bytes</label>
                    <InputNumber
                      :modelValue="ep.maxBodyBytes ?? null"
                      class="full-width"
                      @update:modelValue="(v: number | null) => (ep.maxBodyBytes = v ?? undefined)"
                    />
                    <small v-if="fieldError('endpoints', i, 'maxBodyBytes')" class="field-error">{{ fieldError("endpoints", i, "maxBodyBytes") }}</small>
                  </div>
                  <div class="form-field">
                    <label>Timeout (ms)</label>
                    <InputNumber
                      :modelValue="ep.timeoutMs ?? null"
                      class="full-width"
                      @update:modelValue="(v: number | null) => (ep.timeoutMs = v ?? undefined)"
                    />
                    <small v-if="fieldError('endpoints', i, 'timeoutMs')" class="field-error">{{ fieldError("endpoints", i, "timeoutMs") }}</small>
                  </div>
                </div>
                <div class="checkbox-field">
                  <Checkbox :modelValue="!!ep.cors" binary :inputId="`cors-${i}`" @update:modelValue="toggleCors(ep)" />
                  <label :for="`cors-${i}`">CORS</label>
                </div>
                <div v-if="ep.cors" class="form-grid cors-grid">
                  <div class="form-field">
                    <label>Origins</label>
                    <InputText
                      :modelValue="joinList(ep.cors.origins)"
                      class="full-width"
                      @update:modelValue="(v: string | undefined) => (ep.cors!.origins = parseList(v) ?? [])"
                    />
                    <small class="hint">Comma-separated; "*" or scheme://host[:port].</small>
                  </div>
                  <div class="form-field">
                    <label>Methods</label>
                    <InputText
                      :modelValue="joinList(ep.cors.methods)"
                      class="full-width"
                      @update:modelValue="(v: string | undefined) => (ep.cors!.methods = parseList(v) ?? [])"
                    />
                  </div>
                  <div class="form-field">
                    <label>Headers</label>
                    <InputText
                      :modelValue="joinList(ep.cors.headers)"
                      class="full-width"
                      @update:modelValue="(v: string | undefined) => (ep.cors!.headers = parseList(v) ?? [])"
                    />
                  </div>
                  <div class="form-field checkbox-field">
                    <Checkbox v-model="ep.cors.allowCredentials" binary :inputId="`cors-cred-${i}`" />
                    <label :for="`cors-cred-${i}`">Allow credentials</label>
                  </div>
                </div>
                <Button label="Remove endpoint" icon="pi pi-trash" text size="small" severity="danger" @click="removeEndpoint(i)" />
              </div>
              <Button label="Add endpoint" icon="pi pi-plus" text size="small" data-testid="add-endpoint-button" @click="addEndpoint" />
            </section>

            <section class="editor-section">
              <h3>Subscriptions</h3>
              <Message v-for="(err, idx) in subscriptionErrors" :key="`s${idx}`" severity="error" :closable="false">
                <strong>{{ err.code }}</strong> {{ err.message }}
              </Message>
              <div v-for="(s, i) in model.subscriptions ?? []" :key="i" class="list-row" data-testid="subscription-row">
                <Message v-for="(err, idx) in rowErrors('subscriptions', i)" :key="idx" severity="error" :closable="false">
                  <strong>{{ err.code }}</strong> {{ err.message }}
                </Message>
                <div class="form-grid">
                  <div class="form-field">
                    <label>Event type <span class="required">*</span></label>
                    <InputText v-model="s.eventType" class="full-width" />
                    <small v-if="fieldError('subscriptions', i, 'eventType')" class="field-error">{{ fieldError("subscriptions", i, "eventType") }}</small>
                  </div>
                  <div class="form-field">
                    <label>Path <span class="required">*</span></label>
                    <InputText v-model="s.path" class="full-width" />
                    <small v-if="fieldError('subscriptions', i, 'path')" class="field-error">{{ fieldError("subscriptions", i, "path") }}</small>
                  </div>
                  <div class="form-field">
                    <label>Mode</label>
                    <Select
                      :modelValue="s.mode ?? 'IMMEDIATE'"
                      :options="modeOptions"
                      class="full-width"
                      @update:modelValue="(v: Subscription['mode']) => (s.mode = v)"
                    />
                  </div>
                  <div class="form-field">
                    <label>Max retries</label>
                    <InputNumber
                      :modelValue="s.maxRetries ?? null"
                      class="full-width"
                      @update:modelValue="(v: number | null) => (s.maxRetries = v ?? undefined)"
                    />
                  </div>
                  <div class="form-field">
                    <label>Timeout (s)</label>
                    <InputNumber
                      :modelValue="s.timeoutSeconds ?? null"
                      class="full-width"
                      @update:modelValue="(v: number | null) => (s.timeoutSeconds = v ?? undefined)"
                    />
                  </div>
                  <div class="form-field checkbox-field">
                    <Checkbox
                      :modelValue="!!s.dataOnly"
                      binary
                      :inputId="`data-only-${i}`"
                      @update:modelValue="(v: boolean) => (s.dataOnly = v)"
                    />
                    <label :for="`data-only-${i}`">Data only</label>
                  </div>
                </div>
                <Button label="Remove subscription" icon="pi pi-trash" text size="small" severity="danger" @click="removeSubscription(i)" />
              </div>
              <Button label="Add subscription" icon="pi pi-plus" text size="small" @click="addSubscription" />
            </section>

            <section class="editor-section">
              <h3>Schedules</h3>
              <Message v-for="(err, idx) in scheduleErrors" :key="`s${idx}`" severity="error" :closable="false">
                <strong>{{ err.code }}</strong> {{ err.message }}
              </Message>
              <div v-for="(s, i) in model.schedules ?? []" :key="i" class="list-row">
                <Message v-for="(err, idx) in rowErrors('schedules', i)" :key="idx" severity="error" :closable="false">
                  <strong>{{ err.code }}</strong> {{ err.message }}
                </Message>
                <div class="form-grid">
                  <div class="form-field">
                    <label>Cron <span class="required">*</span></label>
                    <InputText v-model="s.cron" class="full-width" />
                    <small v-if="fieldError('schedules', i, 'cron')" class="field-error">{{ fieldError("schedules", i, "cron") }}</small>
                  </div>
                  <div class="form-field">
                    <label>Timezone</label>
                    <InputText v-model="s.timezone" class="full-width" />
                    <small v-if="fieldError('schedules', i, 'timezone')" class="field-error">{{ fieldError("schedules", i, "timezone") }}</small>
                  </div>
                  <div class="form-field">
                    <label>Path <span class="required">*</span></label>
                    <InputText v-model="s.path" class="full-width" />
                    <small v-if="fieldError('schedules', i, 'path')" class="field-error">{{ fieldError("schedules", i, "path") }}</small>
                  </div>
                  <div class="form-field span-all">
                    <label>Payload (JSON)</label>
                    <Textarea
                      :modelValue="schedulePayloadText(s)"
                      rows="2"
                      class="full-width"
                      @update:modelValue="(v: string | undefined) => setSchedulePayload(s, i, v)"
                    />
                    <small v-if="schedulePayloadErrors[i]" class="field-error">{{ schedulePayloadErrors[i] }}</small>
                  </div>
                </div>
                <Button label="Remove schedule" icon="pi pi-trash" text size="small" severity="danger" @click="removeSchedule(i)" />
              </div>
              <Button label="Add schedule" icon="pi pi-plus" text size="small" @click="addSchedule" />
            </section>

            <section class="editor-section">
              <h3>Public routes</h3>
              <Message v-for="(err, idx) in publicRouteErrors" :key="`s${idx}`" severity="error" :closable="false">
                <strong>{{ err.code }}</strong> {{ err.message }}
              </Message>
              <div v-for="(p, i) in model.public ?? []" :key="i" class="list-row">
                <Message v-for="(err, idx) in rowErrors('public', i)" :key="idx" severity="error" :closable="false">
                  <strong>{{ err.code }}</strong> {{ err.message }}
                </Message>
                <div class="form-grid">
                  <div class="form-field">
                    <label>Hostname <span class="required">*</span></label>
                    <InputText v-model="p.hostname" class="full-width" />
                    <small v-if="fieldError('public', i, 'hostname')" class="field-error">{{ fieldError("public", i, "hostname") }}</small>
                  </div>
                  <div class="form-field">
                    <label>Path prefix</label>
                    <InputText v-model="p.pathPrefix" class="full-width" />
                    <small v-if="fieldError('public', i, 'pathPrefix')" class="field-error">{{ fieldError("public", i, "pathPrefix") }}</small>
                    <small v-else class="hint">Defaults to "/" when absent.</small>
                  </div>
                  <div class="form-field">
                    <label>Alias prefixes</label>
                    <InputText
                      :modelValue="joinList(p.aliasPrefixes)"
                      class="full-width"
                      @update:modelValue="(v: string | undefined) => (p.aliasPrefixes = parseList(v))"
                    />
                    <small v-if="fieldError('public', i, 'aliasPrefixes')" class="field-error">{{ fieldError("public", i, "aliasPrefixes") }}</small>
                    <small v-else class="hint">Comma-separated; opt-in, never "live".</small>
                  </div>
                </div>
                <Button label="Remove route" icon="pi pi-trash" text size="small" severity="danger" @click="removePublicRoute(i)" />
              </div>
              <Button label="Add public route" icon="pi pi-plus" text size="small" @click="addPublicRoute" />
            </section>

            <section class="editor-section">
              <h3>Config, secrets &amp; outbound hosts</h3>
              <div class="form-field">
                <label>Config keys</label>
                <InputText
                  :modelValue="joinList(model.config)"
                  class="full-width"
                  data-testid="manifest-config-input"
                  @update:modelValue="(v: string | undefined) => (model.config = parseList(v))"
                />
                <small v-if="topFieldError('config')" class="field-error">{{ topFieldError("config") }}</small>
                <small v-else class="hint">Comma-separated names of required config values.</small>
              </div>
              <div class="form-field">
                <label>Secret keys</label>
                <InputText
                  :modelValue="joinList(model.secrets)"
                  class="full-width"
                  @update:modelValue="(v: string | undefined) => (model.secrets = parseList(v))"
                />
                <small v-if="topFieldError('secrets')" class="field-error">{{ topFieldError("secrets") }}</small>
                <small v-else class="hint">Comma-separated names of required secrets.</small>
              </div>
              <div class="form-field">
                <label>Outbound hosts (httpAllow)</label>
                <InputText
                  :modelValue="joinList(model.httpAllow)"
                  class="full-width"
                  @update:modelValue="(v: string | undefined) => (model.httpAllow = parseList(v))"
                />
                <small v-if="topFieldError('httpAllow')" class="field-error">{{ topFieldError("httpAllow") }}</small>
                <small v-else class="hint">Comma-separated.</small>
              </div>
            </section>

            <section class="editor-section">
              <h3>Database connections</h3>
              <Message v-for="(err, idx) in dbErrors" :key="`s${idx}`" severity="error" :closable="false">
                <strong>{{ err.code }}</strong> {{ err.message }}
              </Message>
              <div v-for="(d, i) in model.db ?? []" :key="i" class="list-row">
                <Message v-for="(err, idx) in rowErrors('db', i)" :key="idx" severity="error" :closable="false">
                  <strong>{{ err.code }}</strong> {{ err.message }}
                </Message>
                <div class="form-grid">
                  <div class="form-field">
                    <label>Name <span class="required">*</span></label>
                    <InputText v-model="d.name" class="full-width" />
                    <small v-if="fieldError('db', i, 'name')" class="field-error">{{ fieldError("db", i, "name") }}</small>
                  </div>
                  <div class="form-field">
                    <label>Secret ref <span class="required">*</span></label>
                    <InputText v-model="d.secretRef" class="full-width" />
                    <small v-if="fieldError('db', i, 'secretRef')" class="field-error">{{ fieldError("db", i, "secretRef") }}</small>
                  </div>
                  <div class="form-field">
                    <label>Pool size</label>
                    <InputNumber
                      :modelValue="d.poolSize ?? null"
                      class="full-width"
                      @update:modelValue="(v: number | null) => (d.poolSize = v ?? undefined)"
                    />
                    <small v-if="fieldError('db', i, 'poolSize')" class="field-error">{{ fieldError("db", i, "poolSize") }}</small>
                  </div>
                </div>
                <Button label="Remove connection" icon="pi pi-trash" text size="small" severity="danger" @click="removeDbRef(i)" />
              </div>
              <Button label="Add connection" icon="pi pi-plus" text size="small" @click="addDbRef" />
            </section>
          </fieldset>
        </div>
      </div>

      <div class="fc-card validate-card">
        <h3 class="validate-title">Validate</h3>
        <div class="validate-row">
          <div class="form-field">
            <label for="check-alias">Alias</label>
            <InputText id="check-alias" v-model="checkAlias" class="alias-input" data-testid="manifest-check-alias-input" />
          </div>
          <Button
            label="Validate"
            icon="pi pi-check-circle"
            :loading="validating"
            :disabled="formReadOnly"
            data-testid="manifest-validate-button"
            @click="validate"
          />
        </div>

        <Message v-if="checkTransportError" severity="error" :closable="false">{{ checkTransportError }}</Message>

        <template v-if="checkResult">
          <Message v-if="!checkResult.valid" severity="error" :closable="false" data-testid="manifest-errors">
            <ul class="check-list">
              <li v-for="(err, idx) in checkResult.errors" :key="idx">
                <strong>{{ err.code }}</strong>
                <span v-if="errorPointer(err)" class="font-mono"> {{ errorPointer(err) }}</span>: {{ err.message }}
              </li>
            </ul>
          </Message>
          <Message v-else severity="success" :closable="false" data-testid="manifest-plan">
            <p class="plan-heading">Valid. Promote plan for "{{ checkAlias.trim() || "live" }}":</p>
            <ul class="check-list plan-lines">
              <li v-for="(line, idx) in planLines" :key="idx">{{ line }}</li>
            </ul>
          </Message>
        </template>
      </div>
    </template>

    <PublishVersionDialog
      v-if="showPublishDialog"
      :address="address"
      :initial-manifest="model"
      @close="showPublishDialog = false"
      @published="onPublished"
    />
  </div>
</template>

<style scoped>
.header-content {
  display: flex;
  align-items: flex-start;
  gap: 16px;
}

.header-actions {
  display: flex;
  gap: 8px;
}

.loading-container {
  display: flex;
  justify-content: center;
  padding: 60px;
}

.editor-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  margin-bottom: 16px;
}

.editor-toolbar-actions {
  display: flex;
  align-items: center;
  gap: 12px;
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

.manifest-json-textarea {
  width: 100%;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 12px;
}

.form-disabled {
  opacity: 0.6;
}

.form-fieldset {
  border: none;
  margin: 0;
  padding: 0;
  min-width: 0;
}

.editor-section {
  padding: 16px 0;
  border-top: 1px solid var(--surface-border);
}

.editor-section:first-child {
  border-top: none;
  padding-top: 0;
}

.editor-section h3 {
  margin: 0 0 12px;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-color-secondary);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.form-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 12px 16px;
}

.form-field {
  margin-bottom: 12px;
  min-width: 0;
}

.form-field > label {
  display: block;
  font-weight: 500;
  margin-bottom: 6px;
  font-size: 0.875rem;
}

.span-all {
  grid-column: 1 / -1;
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

.full-width {
  width: 100%;
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
  font-size: 12px;
  color: #dc2626;
}

.list-row {
  padding: 12px;
  margin-bottom: 12px;
  border: 1px dashed var(--surface-border);
  border-radius: 6px;
}

.cors-grid {
  margin-top: 8px;
}

.validate-card {
  margin-top: 16px;
}

.validate-title {
  margin: 0 0 12px;
  font-size: 1rem;
  font-weight: 600;
}

.validate-row {
  display: flex;
  align-items: flex-end;
  gap: 12px;
  margin-bottom: 12px;
}

.validate-row .form-field {
  margin-bottom: 0;
}

.alias-input {
  width: 10rem;
}

.check-list {
  margin: 4px 0 0;
  padding-left: 18px;
}

.plan-lines li,
.font-mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.plan-lines li {
  font-size: 12px;
}

.plan-heading {
  margin: 0 0 4px;
  font-weight: 600;
}
</style>
