<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed } from "vue";
import { dispatchPoolsApi } from "@/api/dispatch-pools";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";

const emit = defineEmits<{
	changed: [];
}>();

// Form fields
const code = ref("");
const name = ref("");
const description = ref("");
const rateLimit = ref<number | null>(null);
const concurrency = ref<number>(10);
const clientId = ref<string | null>(null);
const isAnchorLevel = ref(false);

// Cheap dirty check: anything typed or selected counts.
const dirty = computed(
	() =>
		code.value !== "" ||
		name.value !== "" ||
		description.value !== "" ||
		rateLimit.value !== null ||
		clientId.value !== null,
);

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { goToList, replaceToDetail } = useDrawerRoute({
	listPath: "/dispatch-pools",
	dirty,
});

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
		(rateLimit.value === null || rateLimit.value >= 1) &&
		concurrency.value >= 1
	);
});

async function onSubmit() {
	if (!isFormValid.value) return;

	submitting.value = true;
	errorMessage.value = null;

	try {
		const pool = await dispatchPoolsApi.create({
			code: code.value,
			name: name.value,
			description: description.value || undefined,
			rateLimit: rateLimit.value ?? undefined,
			concurrency: concurrency.value,
			clientId: isAnchorLevel.value ? undefined : clientId.value || undefined,
		});
		toast.success("Success", "Dispatch pool created");
		emit("changed");
		replaceToDetail(pool.id);
	} catch (e) {
		errorMessage.value =
			e instanceof Error ? e.message : "Failed to create dispatch pool";
	} finally {
		submitting.value = false;
	}
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    title="Create Dispatch Pool"
    subtitle="Configure a new pool for dispatch jobs"
    :dirty="dirty"
    @close="goToList()"
  >
    <div class="form-section">
      <h3>Basic Information</h3>

      <div class="form-field">
        <label>Code <span class="required">*</span></label>
        <InputText
          v-model="code"
          placeholder="pool-code"
          class="full-width"
          :invalid="!!(code && !isCodeValid)"
        />
        <small v-if="code && !isCodeValid" class="p-error">
          Lowercase letters, numbers, hyphens only. Must start with a letter.
        </small>
        <small v-else class="hint"> Unique identifier for this pool (2-100 characters) </small>
      </div>

      <div class="form-field">
        <label>Name <span class="required">*</span></label>
        <InputText
          v-model="name"
          placeholder="Pool display name"
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
      <h3>Rate Limiting</h3>

      <div class="form-row">
        <div class="form-field">
          <label>Rate Limit (per minute)</label>
          <InputNumber v-model="rateLimit" :min="1" class="full-width" placeholder="Unlimited" />
          <small class="hint">Optional. Leave blank to run on concurrency only.</small>
        </div>

        <div class="form-field">
          <label>Concurrency <span class="required">*</span></label>
          <InputNumber v-model="concurrency" :min="1" class="full-width" />
          <small class="hint">Maximum concurrent dispatches</small>
        </div>
      </div>
    </div>

    <div class="form-section">
      <h3>Scope</h3>

      <div class="form-field">
        <div class="checkbox-field">
          <Checkbox v-model="isAnchorLevel" :binary="true" inputId="anchorLevel" />
          <label for="anchorLevel">Anchor-level pool (not client-scoped)</label>
        </div>
        <small class="hint">
          Anchor-level pools are for dispatch jobs that are not scoped to a specific client.
        </small>
      </div>

      <div v-if="!isAnchorLevel" class="form-field">
        <label>Client</label>
        <ClientSelect v-model="clientId" placeholder="Search for a client (optional)" />
        <small class="hint">
          If specified, this pool will only be used for jobs scoped to this client.
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
          label="Create Pool"
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

.form-row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 20px;
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

@media (max-width: 640px) {
  .form-row {
    grid-template-columns: 1fr;
  }
}
</style>
