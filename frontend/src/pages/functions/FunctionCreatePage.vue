<script setup lang="ts">
// Create a function address. Ownership follows the platform's create rule: a
// client-scoped caller's function is always its own client's (sent
// automatically); only an unscoped caller chooses platform or a client.
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { toast } from "@/utils/errorBus";
import { ApiError } from "@/api/client";
import { functionsApi, type FunctionRuntime } from "@/api/functions";
import { applicationsApi, type Application } from "@/api/applications";
import { useAuthStore } from "@/stores/auth";
import { isUnscopedUser } from "@/stores/permissions";
import { DNS_LABEL_PATTERN, detailErrors } from "./format";

const router = useRouter();
const authStore = useAuthStore();
const unscoped = computed(() => isUnscopedUser(authStore.user));

const applications = ref<Application[]>([]);
const applicationCode = ref("");
const serviceName = ref("");
const name = ref("");
const description = ref("");
const runtime = ref<FunctionRuntime>("wasm");
const runtimeOptions: Array<{ label: string; value: FunctionRuntime }> = [
	{ label: "WASM", value: "wasm" },
	{ label: "JVM", value: "jvm" },
];

const platformOwned = ref(true);
const clientId = ref<string | null>(null);

const submitting = ref(false);
const errorCode = ref<string | null>(null);
const errorMessage = ref<string | null>(null);
const fieldErrors = ref<Array<{ location?: string; message: string }>>([]);

const applicationOptions = computed(() =>
	applications.value.map((a) => ({ label: `${a.code} — ${a.name}`, value: a.code })),
);

const addressPreview = computed(() => {
	if (!applicationCode.value || !serviceName.value || !name.value) return null;
	return `${applicationCode.value}.${serviceName.value}.${name.value}`;
});

function labelInvalid(value: string): boolean {
	return value !== "" && !DNS_LABEL_PATTERN.test(value);
}

const isFormValid = computed(
	() =>
		DNS_LABEL_PATTERN.test(applicationCode.value) &&
		DNS_LABEL_PATTERN.test(serviceName.value) &&
		DNS_LABEL_PATTERN.test(name.value) &&
		(!unscoped.value || platformOwned.value || !!clientId.value),
);

onMounted(async () => {
	try {
		applications.value = (await applicationsApi.list()).applications;
	} catch (err) {
		console.error("Failed to load applications", err);
	}
});

function ownerClientId(): string | undefined {
	if (!unscoped.value) return authStore.user?.clientId ?? undefined;
	return platformOwned.value ? undefined : (clientId.value ?? undefined);
}

async function onSubmit() {
	if (!isFormValid.value) return;
	submitting.value = true;
	errorCode.value = null;
	errorMessage.value = null;
	fieldErrors.value = [];
	try {
		const fn = await functionsApi.create(
			{
				applicationCode: applicationCode.value,
				serviceName: serviceName.value,
				name: name.value,
				runtime: runtime.value,
				description: description.value || undefined,
				clientId: ownerClientId(),
			},
			{ suppressGlobalErrorToast: true },
		);
		toast.success("Success", `Function ${fn.address} created`);
		await router.replace(`/functions/${encodeURIComponent(fn.address)}`);
	} catch (e) {
		if (e instanceof ApiError) {
			errorCode.value = e.code ?? null;
			errorMessage.value = e.message;
			fieldErrors.value = detailErrors(e.details);
		} else {
			errorMessage.value = e instanceof Error ? e.message : "Failed to create function";
		}
	} finally {
		submitting.value = false;
	}
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Create Function</h1>
        <p class="page-subtitle">Register a new function address</p>
      </div>
    </header>

    <form @submit.prevent="onSubmit">
      <div class="form-card">
        <div class="form-section">
          <h3>Address</h3>

          <div class="form-row">
            <div class="form-field">
              <label for="fn-application">Application <span class="required">*</span></label>
              <Select
                v-model="applicationCode"
                inputId="fn-application"
                :options="applicationOptions"
                optionLabel="label"
                optionValue="value"
                placeholder="acme"
                editable
                filter
                class="full-width"
                :invalid="labelInvalid(applicationCode)"
              />
            </div>
            <div class="form-field">
              <label for="fn-service">Service <span class="required">*</span></label>
              <InputText
                id="fn-service"
                v-model="serviceName"
                placeholder="default"
                class="full-width"
                :invalid="labelInvalid(serviceName)"
              />
            </div>
            <div class="form-field">
              <label for="fn-name">Name <span class="required">*</span></label>
              <InputText
                id="fn-name"
                v-model="name"
                placeholder="hello"
                class="full-width"
                :invalid="labelInvalid(name)"
              />
            </div>
          </div>
          <small class="hint">
            Each part is 1-63 characters of a-z, 0-9 and '-', not starting or ending with '-'.
          </small>

          <div class="address-preview">
            <span class="address-preview-label">Address:</span>
            <code v-if="addressPreview">{{ addressPreview }}</code>
            <span v-else class="unset">—</span>
          </div>
        </div>

        <div class="form-section">
          <h3>Runtime</h3>
          <div class="form-field">
            <label for="fn-runtime">Runtime <span class="required">*</span></label>
            <Select
              v-model="runtime"
              inputId="fn-runtime"
              :options="runtimeOptions"
              optionLabel="label"
              optionValue="value"
              class="full-width"
            />
            <small v-if="runtime === 'wasm'" class="hint">
              On a Rust function host, a WASM function is a WASI 0.2 component exporting
              <code>wasi:http/incoming-handler</code> (entrypoint
              <code>wasi_http_incoming_handler</code>). Core Wasm modules (the Extism style) run on
              Java hosts only; pools keep the two apart.
            </small>
            <small v-else class="hint">A JVM function is a jar, run by a Java function host.</small>
          </div>
        </div>

        <div class="form-section">
          <h3>Details</h3>
          <div class="form-field">
            <label for="fn-description">Description</label>
            <Textarea
              id="fn-description"
              v-model="description"
              class="full-width"
              rows="3"
              placeholder="Optional description..."
            />
          </div>
        </div>

        <div v-if="unscoped" class="form-section">
          <h3>Owner</h3>
          <div class="form-field">
            <div class="checkbox-field">
              <Checkbox v-model="platformOwned" :binary="true" inputId="platformOwned" />
              <label for="platformOwned">Platform-owned function</label>
            </div>
          </div>
          <div v-if="!platformOwned" class="form-field">
            <label>Client <span class="required">*</span></label>
            <ClientSelect v-model="clientId" placeholder="Search for a client" />
          </div>
        </div>

        <Message v-if="errorMessage" severity="error" class="error-message">
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

        <div class="form-actions">
          <Button
            label="Cancel"
            icon="pi pi-times"
            severity="secondary"
            outlined
            :disabled="submitting"
            @click="router.push('/functions')"
          />
          <Button
            label="Create Function"
            icon="pi pi-check"
            type="submit"
            :loading="submitting"
            :disabled="!isFormValid"
          />
        </div>
      </div>
    </form>
  </div>
</template>

<style scoped>
.page-container {
  max-width: 800px;
}

.form-card {
  background: var(--surface-card, white);
  border-radius: 8px;
  border: 1px solid var(--surface-border);
  padding: 24px;
}

.form-section {
  margin-bottom: 32px;
}

.form-section h3 {
  margin: 0 0 16px 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-color-secondary);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.form-field {
  margin-bottom: 20px;
}

.form-field > label {
  display: block;
  font-weight: 500;
  margin-bottom: 6px;
}

.required {
  color: #ef4444;
}

.form-row {
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  gap: 20px;
}

.full-width {
  width: 100%;
}

.hint {
  display: block;
  font-size: 12px;
  color: var(--text-color-secondary);
  margin-top: 4px;
}

.address-preview {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 12px;
  font-size: 13px;
}

.address-preview-label {
  font-weight: 500;
}

.unset {
  color: var(--text-color-secondary);
  font-style: italic;
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

.error-message {
  margin-bottom: 16px;
}

.field-errors {
  margin: 8px 0 0;
  padding-left: 18px;
  font-size: 13px;
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: 12px;
  padding-top: 16px;
  border-top: 1px solid var(--surface-border);
}

@media (max-width: 640px) {
  .form-row {
    grid-template-columns: 1fr;
  }
}
</style>
