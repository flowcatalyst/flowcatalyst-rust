<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, onMounted } from "vue";
import { connectionsApi } from "@/api/connections";
import {
	serviceAccountsApi,
	type ServiceAccount,
} from "@/api/service-accounts";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";

const emit = defineEmits<{
	changed: [];
}>();

// Form fields
const code = ref("");
const name = ref("");
const description = ref("");
const externalId = ref("");
const serviceAccountId = ref<string | null>(null);
const clientId = ref<string | null>(null);

// Cheap dirty check: anything typed or selected counts.
const dirty = computed(
	() =>
		code.value !== "" ||
		name.value !== "" ||
		description.value !== "" ||
		externalId.value !== "" ||
		serviceAccountId.value !== null,
);

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { goToList, replaceToDetail } = useDrawerRoute({
	listPath: "/connections",
	dirty,
});

// Lookup data
const serviceAccounts = ref<ServiceAccount[]>([]);
const loadingServiceAccounts = ref(true);
const submitting = ref(false);
const errorMessage = ref<string | null>(null);

const CODE_PATTERN = /^[a-z][a-z0-9-]*$/;

const isCodeValid = computed(() => {
	return !code.value || CODE_PATTERN.test(code.value);
});

const isFormValid = computed(() => {
	return (
		code.value.length >= 2 &&
		code.value.length <= 100 &&
		CODE_PATTERN.test(code.value) &&
		name.value.trim().length > 0 &&
		name.value.length <= 255 &&
		serviceAccountId.value !== null
	);
});

onMounted(async () => {
	await loadServiceAccounts();
});

async function loadServiceAccounts() {
	loadingServiceAccounts.value = true;
	try {
		const response = await serviceAccountsApi.list();
		serviceAccounts.value = response.serviceAccounts;
	} catch (e) {
		console.error("Failed to load service accounts:", e);
	} finally {
		loadingServiceAccounts.value = false;
	}
}

async function onSubmit() {
	if (!isFormValid.value) return;

	submitting.value = true;
	errorMessage.value = null;

	try {
		const connection = await connectionsApi.create({
			code: code.value,
			name: name.value,
			description: description.value || undefined,
			externalId: externalId.value || undefined,
			serviceAccountId: serviceAccountId.value!,
			clientId: clientId.value || undefined,
		});
		toast.success("Success", "Connection created");
		emit("changed");
		replaceToDetail(connection.id);
	} catch (e) {
		errorMessage.value =
			e instanceof Error ? e.message : "Failed to create connection";
	} finally {
		submitting.value = false;
	}
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    title="Create Connection"
    subtitle="Configure a new webhook connection for event delivery"
    :dirty="dirty"
    @close="goToList()"
  >
    <div class="form-section">
      <h3>Basic Information</h3>

      <div class="form-field">
        <label>Code <span class="required">*</span></label>
        <InputText
          v-model="code"
          placeholder="connection-code"
          class="full-width"
          :invalid="!!(code && !isCodeValid)"
        />
        <small v-if="code && !isCodeValid" class="p-error">
          Lowercase letters, numbers, hyphens only. Must start with a letter.
        </small>
        <small v-else class="hint">
          Unique identifier for this connection (2-100 characters)
        </small>
      </div>

      <div class="form-field">
        <label>Name <span class="required">*</span></label>
        <InputText
          v-model="name"
          placeholder="Connection display name"
          class="full-width"
          :invalid="name.length > 255"
        />
        <small class="char-count">{{ name.length }} / 255</small>
      </div>

      <div class="form-field">
        <label>Description</label>
        <Textarea
          v-model="description"
          placeholder="Optional description..."
          class="full-width"
          rows="3"
        />
      </div>
    </div>

    <div class="form-section">
      <h3>Additional</h3>

      <div class="form-field">
        <label>External ID</label>
        <InputText
          v-model="externalId"
          placeholder="Optional external identifier"
          class="full-width"
        />
        <small class="hint">
          An optional external reference for this connection
        </small>
      </div>
    </div>

    <div class="form-section">
      <h3>Service Account</h3>

      <div class="form-field">
        <label>Service Account <span class="required">*</span></label>
        <Select
          v-model="serviceAccountId"
          :options="serviceAccounts"
          optionLabel="name"
          optionValue="id"
          placeholder="Select a service account"
          class="full-width"
          :loading="loadingServiceAccounts"
          :disabled="loadingServiceAccounts"
          filter
        >
          <template #option="{ option }">
            <div class="dropdown-option">
              <span class="option-name">{{ option.name }}</span>
              <span class="option-code">{{ option.code }}</span>
            </div>
          </template>
        </Select>
        <small class="hint">
          The service account used for authentication when delivering events
        </small>
      </div>
    </div>

    <div class="form-section">
      <h3>Scope</h3>

      <div class="form-field">
        <label>Client</label>
        <ClientSelect
          v-model="clientId"
          placeholder="Anchor-level (leave empty) or select a client"
        />
        <small class="hint">
          Leave empty for an anchor-level connection, or select a specific client.
        </small>
      </div>
    </div>

    <Message v-if="errorMessage" severity="error" class="error-message">
      {{ errorMessage }}
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
          label="Create Connection"
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
.form-section {
  margin-bottom: 32px;
}

.form-section h3 {
  margin: 0 0 16px 0;
  font-size: 14px;
  font-weight: 600;
  color: #475569;
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

.form-field .required {
  color: #ef4444;
}

.full-width {
  width: 100%;
}

.char-count {
  display: block;
  text-align: right;
  font-size: 12px;
  color: #94a3b8;
  margin-top: 4px;
}

.hint {
  display: block;
  font-size: 12px;
  color: #64748b;
  margin-top: 4px;
}

.dropdown-option {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.option-name {
  font-weight: 500;
}

.option-code {
  font-size: 12px;
  color: #64748b;
  font-family: monospace;
}

.error-message {
  margin-bottom: 16px;
}
</style>
