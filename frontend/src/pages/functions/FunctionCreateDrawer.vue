<script setup lang="ts">
// Create a function address. Ownership follows the platform's create rule: a
// client-scoped caller's function is always its own client's (sent
// automatically); only an unscoped caller chooses platform or a client.
// On success the new function opens in its full detail page (versions,
// config, invoke and routes need the room a drawer doesn't have).
import { computed, onMounted, ref } from "vue";
import { toast } from "@/utils/errorBus";
import { ApiError } from "@/api/client";
import { functionsApi, type FunctionRuntime } from "@/api/functions";
import { applicationsApi, type Application } from "@/api/applications";
import { useAuthStore } from "@/stores/auth";
import { isUnscopedUser } from "@/stores/permissions";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";
import { DNS_LABEL_PATTERN, detailErrors } from "./format";

const emit = defineEmits<{
	changed: [];
}>();

const authStore = useAuthStore();
const unscoped = computed(() => isUnscopedUser(authStore.user));

const applications = ref<Application[]>([]);
const applicationCode = ref("");
const serviceName = ref("");
const name = ref("");
const description = ref("");
const runtime = ref<FunctionRuntime>("component");
const runtimeOptions: Array<{ label: string; value: FunctionRuntime }> = [
	{ label: "Component (WASI 0.2)", value: "component" },
	{ label: "WASM", value: "wasm" },
	{ label: "JVM", value: "jvm" },
];

const platformOwned = ref(true);
const clientId = ref<string | null>(null);

// Cheap dirty check: anything typed or selected counts.
const dirty = computed(
	() =>
		applicationCode.value !== "" ||
		serviceName.value !== "" ||
		name.value !== "" ||
		description.value !== "" ||
		clientId.value !== null,
);

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { goToList, replaceToDetail } = useDrawerRoute({ listPath: "/functions", dirty });

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
		emit("changed");
		// /functions/:address is the full detail page, a sibling route.
		replaceToDetail(encodeURIComponent(fn.address));
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
  <EntityDrawer
    ref="drawer"
    title="Create Function"
    subtitle="Register a new function address"
    :dirty="dirty"
    @close="goToList()"
  >
    <FcFormSection title="Address" flat>
      <div class="fc-form-grid address-grid">
        <FcFormField label="Application" required>
          <template #default="{ id: fieldId }">
            <Select
              v-model="applicationCode"
              :inputId="fieldId"
              :options="applicationOptions"
              optionLabel="label"
              optionValue="value"
              placeholder="acme"
              editable
              filter
              :invalid="labelInvalid(applicationCode)"
            />
          </template>
        </FcFormField>
        <FcFormField label="Service" required>
          <template #default="{ id: fieldId }">
            <InputText
              :id="fieldId"
              v-model="serviceName"
              placeholder="default"
              :invalid="labelInvalid(serviceName)"
            />
          </template>
        </FcFormField>
        <FcFormField label="Name" required>
          <template #default="{ id: fieldId }">
            <InputText
              :id="fieldId"
              v-model="name"
              placeholder="hello"
              :invalid="labelInvalid(name)"
            />
          </template>
        </FcFormField>
      </div>
      <small class="fc-field-help">
        Each part is 1-63 characters of a-z, 0-9 and '-', not starting or ending with '-'.
      </small>
      <div class="address-preview">
        <span class="address-preview-label">Address:</span>
        <code v-if="addressPreview">{{ addressPreview }}</code>
        <span v-else class="unset">—</span>
      </div>
    </FcFormSection>

    <FcFormSection title="Runtime" flat>
      <FcFormField label="Runtime" required>
        <template #default="{ id: fieldId }">
          <Select
            v-model="runtime"
            :inputId="fieldId"
            :options="runtimeOptions"
            optionLabel="label"
            optionValue="value"
          />
        </template>
        <template #help>
          <template v-if="runtime === 'component'">
            A WASI 0.2 component exporting <code>wasi:http/incoming-handler</code>, run by a Rust
            function host. The platform checks an uploaded artifact is a component when you
            publish.
          </template>
          <template v-else-if="runtime === 'wasm'">
            On a Rust function host, a WASM function is a WASI 0.2 component exporting
            <code>wasi:http/incoming-handler</code> (entrypoint
            <code>wasi_http_incoming_handler</code>). Core Wasm modules (the Extism style) run on
            Java hosts only; pools keep the two apart.
          </template>
          <template v-else>A JVM function is a jar, run by a Java function host.</template>
        </template>
      </FcFormField>
    </FcFormSection>

    <FcFormSection title="Details" flat>
      <FcFormField label="Description">
        <template #default="{ id: fieldId }">
          <Textarea
            :id="fieldId"
            v-model="description"
            rows="3"
            placeholder="Optional description..."
          />
        </template>
      </FcFormField>
    </FcFormSection>

    <FcFormSection v-if="unscoped" title="Owner" flat>
      <div class="checkbox-field">
        <Checkbox v-model="platformOwned" :binary="true" inputId="platformOwned" />
        <label for="platformOwned">Platform-owned function</label>
      </div>
      <FcFormField v-if="!platformOwned" label="Client" required>
        <ClientSelect v-model="clientId" placeholder="Search for a client" />
      </FcFormField>
    </FcFormSection>

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

    <template #footer>
      <FcFormActions :bordered="false">
        <Button
          label="Cancel"
          icon="pi pi-times"
          severity="secondary"
          outlined
          :disabled="submitting"
          @click="drawer?.close()"
        />
        <Button
          label="Create Function"
          icon="pi pi-check"
          :loading="submitting"
          :disabled="!isFormValid"
          @click="onSubmit"
        />
      </FcFormActions>
    </template>
  </EntityDrawer>
</template>

<style scoped>
.address-grid {
  grid-template-columns: repeat(3, minmax(0, 1fr));
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
  margin-bottom: 12px;
}

.checkbox-field label {
  margin: 0;
  cursor: pointer;
}

.error-message {
  margin-top: 16px;
}

.field-errors {
  margin: 8px 0 0;
  padding-left: 18px;
  font-size: 13px;
}
</style>
