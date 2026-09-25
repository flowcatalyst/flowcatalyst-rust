<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed } from "vue";
import { applicationsApi, type ApplicationType } from "@/api/applications";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";

const emit = defineEmits<{
	changed: [];
}>();

// Form state
const code = ref("");
const name = ref("");
const description = ref("");
const defaultBaseUrl = ref("");
const iconUrl = ref("");
const website = ref("");
const logo = ref("");
const logoMimeType = ref("");
const type = ref<ApplicationType>("APPLICATION");

// Cheap dirty check: anything typed into the identity fields counts.
const dirty = computed(() => code.value !== "" || name.value !== "");

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { goToList, replaceToDetail } = useDrawerRoute({
	listPath: "/applications",
	dirty,
});

const typeOptions = [
	{ label: "Application", value: "APPLICATION" },
	{ label: "Integration", value: "INTEGRATION" },
];

const submitting = ref(false);
const errorMessage = ref<string | null>(null);

// Validation
const CODE_PATTERN = /^[a-z][a-z0-9-]*$/;

const isCodeValid = computed(
	() => !code.value || CODE_PATTERN.test(code.value),
);

const isFormValid = computed(() => {
	return (
		code.value &&
		CODE_PATTERN.test(code.value) &&
		name.value.trim().length > 0 &&
		name.value.length <= 100
	);
});

async function onSubmit() {
	if (!isFormValid.value) return;

	submitting.value = true;
	errorMessage.value = null;

	try {
		const application = await applicationsApi.create({
			code: code.value,
			name: name.value,
			description: description.value || undefined,
			defaultBaseUrl: defaultBaseUrl.value || undefined,
			iconUrl: iconUrl.value || undefined,
			website: website.value || undefined,
			logo: logo.value || undefined,
			logoMimeType: logoMimeType.value || undefined,
			type: type.value,
		});

		// POST /applications returns the created envelope `{ id }` only —
		// service-account credentials are provisioned (and shown) from the
		// detail drawer, not at create time.
		toast.success("Success", "Application created");
		emit("changed");
		replaceToDetail(application.id);
	} catch (e) {
		errorMessage.value =
			e instanceof Error ? e.message : "Failed to create application";
	} finally {
		submitting.value = false;
	}
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    title="Create Application"
    subtitle="Add a new application to the platform"
    :dirty="dirty"
    @close="goToList()"
  >
    <section class="form-section">
      <h3 class="section-title">Application Identity</h3>

      <div class="form-field">
        <label>Type <span class="required">*</span></label>
        <SelectButton
          v-model="type"
          :options="typeOptions"
          optionLabel="label"
          optionValue="value"
        />
        <small class="hint">
          {{
            type === 'APPLICATION'
              ? 'User-facing application that users can log into'
              : 'Third-party adapter or connector for integrations'
          }}
        </small>
      </div>

      <div class="form-field">
        <label>Code <span class="required">*</span></label>
        <InputText
          v-model="code"
          placeholder="e.g., operant"
          class="full-width"
          :invalid="!!(code && !isCodeValid)"
        />
        <small v-if="code && !isCodeValid" class="p-error">
          Must start with a letter, use only lowercase letters, numbers, and hyphens
        </small>
        <small v-else class="hint">
          Unique identifier for the application. Cannot be changed after creation.
        </small>
      </div>

      <div class="form-field">
        <label>Name <span class="required">*</span></label>
        <InputText
          v-model="name"
          placeholder="Human-friendly name"
          class="full-width"
          :invalid="name.length > 100"
        />
        <small class="char-count">{{ name.length }} / 100</small>
      </div>

      <div class="form-field">
        <label>Description</label>
        <Textarea
          v-model="description"
          placeholder="Optional description"
          :rows="3"
          class="full-width"
        />
      </div>
    </section>

    <section class="form-section">
      <h3 class="section-title">Configuration</h3>

      <div class="form-field">
        <label>Default Base URL</label>
        <InputText
          v-model="defaultBaseUrl"
          placeholder="https://example.com"
          class="full-width"
        />
        <small class="hint">Base URL for API calls to this application</small>
      </div>

      <div class="form-field">
        <label>Icon URL</label>
        <InputText
          v-model="iconUrl"
          placeholder="https://example.com/icon.png"
          class="full-width"
        />
        <small class="hint">URL to the application's icon image</small>
      </div>

      <div class="form-field">
        <label>Website</label>
        <InputText v-model="website" placeholder="https://www.example.com" class="full-width" />
        <small class="hint">Public website URL for this application</small>
      </div>

      <div class="form-field">
        <label>Logo (SVG)</label>
        <Textarea
          v-model="logo"
          placeholder="Paste SVG content here"
          :rows="4"
          class="full-width"
        />
        <small class="hint">SVG logo content to embed in the platform</small>
      </div>

      <div class="form-field" v-if="logo">
        <label>Logo MIME Type</label>
        <InputText v-model="logoMimeType" placeholder="image/svg+xml" class="full-width" />
        <small class="hint">MIME type of the logo (e.g., image/svg+xml)</small>
      </div>
    </section>

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
          label="Create Application"
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

.section-title {
  font-size: 16px;
  font-weight: 600;
  margin-bottom: 16px;
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

.hint {
  display: block;
  font-size: 12px;
  color: #64748b;
  margin-top: 4px;
}

.char-count {
  display: block;
  text-align: right;
  font-size: 12px;
  color: #94a3b8;
  margin-top: 4px;
}

.error-message {
  margin-bottom: 16px;
}
</style>
