<script setup lang="ts">
import { ref, onMounted } from "vue";
import { configApi } from "@/api/config";
import { usePlatformConfigStore } from "@/stores/platformConfig";
import { toast } from "@/utils/errorBus";
import { getErrorMessage } from "@/utils/errors";

const platformConfig = usePlatformConfigStore();

const DEFAULT_NAME = "FlowCatalyst";
const platformName = ref(DEFAULT_NAME);
const loading = ref(true);
const saving = ref(false);
const error = ref("");

onMounted(load);

async function load() {
	loading.value = true;
	try {
		const stored = await configApi.getPlatformName();
		platformName.value = stored?.trim() || DEFAULT_NAME;
	} catch (e) {
		// No value yet → default. Only surface real errors.
		error.value = getErrorMessage(e, "Could not load the platform name.");
	} finally {
		loading.value = false;
	}
}

async function save() {
	error.value = "";
	const name = platformName.value.trim();
	if (!name) {
		error.value = "Platform name cannot be empty.";
		return;
	}
	saving.value = true;
	try {
		await configApi.setPlatformName(name);
		// Refresh the global config so the title/brand update without a reload.
		await platformConfig.loadConfig(true);
		toast.success("Saved", "Platform name updated.");
	} catch (e) {
		error.value = getErrorMessage(e, "Could not save the platform name.");
	} finally {
		saving.value = false;
	}
}

function resetToDefault() {
	platformName.value = DEFAULT_NAME;
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Names</h1>
        <p class="page-subtitle">Set the name used across the platform</p>
      </div>
    </header>

    <div v-if="loading" class="loading-container">
      <ProgressSpinner strokeWidth="3" />
    </div>

    <div v-else class="fc-form">
      <FcFormSection title="Platform Name">
        <FcFormField
          label="Platform name"
          :error="error || undefined"
          help="Shown wherever the product is named to users: security emails, the authenticator-app label for two-factor codes, passkey prompts, and the browser tab. Defaults to “FlowCatalyst”."
        >
          <template #default="{ id: fieldId }">
            <InputText
              :id="fieldId"
              v-model="platformName"
              placeholder="FlowCatalyst"
              @keyup.enter="save"
            />
          </template>
        </FcFormField>

        <FcFormActions>
          <Button
            label="Reset to default"
            text
            severity="secondary"
            icon="pi pi-replay"
            @click="resetToDefault"
          />
          <Button label="Save" icon="pi pi-check" :loading="saving" @click="save" />
        </FcFormActions>

        <p class="note">
          The authenticator-app and email names update immediately. The passkey
          prompt name updates on the next server restart.
        </p>
      </FcFormSection>
    </div>
  </div>
</template>

<style scoped>
.loading-container {
  display: flex;
  justify-content: center;
  padding: 60px;
}

.note {
  margin: 16px 0 0;
  font-size: 12px;
  color: #94a3b8;
}
</style>
