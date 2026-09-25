<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, onMounted } from "vue";
import { configApi, defaultLoginTheme, type LoginTheme } from "@/api/config";
import { getErrorMessage } from "@/utils/errors";
import LoginThemeEditor from "@/components/theme/LoginThemeEditor.vue";

const loading = ref(true);
const saving = ref(false);
const error = ref<string | null>(null);

// Form state
const theme = ref<LoginTheme>(defaultLoginTheme());

onMounted(async () => {
	await loadTheme();
});

async function loadTheme() {
	loading.value = true;
	error.value = null;

	try {
		// Try to get existing theme config
		const themeJson = await configApi.getLoginThemeConfig();
		if (themeJson) {
			theme.value = { ...theme.value, ...JSON.parse(themeJson) };
		}
	} catch {
		// No existing config, use defaults
		console.log("No existing theme config, using defaults");
	} finally {
		loading.value = false;
	}
}

async function saveTheme() {
	saving.value = true;
	error.value = null;

	try {
		await configApi.setLoginThemeConfig(theme.value);
		toast.success("Success", "Theme saved successfully");
	} catch (e: unknown) {
		error.value = getErrorMessage(e, "Failed to save theme");
	} finally {
		saving.value = false;
	}
}

function resetToDefaults() {
	theme.value = defaultLoginTheme();
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Theme Settings</h1>
        <p class="page-subtitle">Customize the appearance of the platform with your branding.</p>
      </div>
    </header>

    <Message v-if="error" severity="error" class="error-message">{{ error }}</Message>

    <div v-if="loading" class="loading-container">
      <ProgressSpinner strokeWidth="3" />
    </div>

    <LoginThemeEditor v-else v-model="theme">
      <template #actions>
        <FcFormActions>
          <Button label="Reset to Defaults" text @click="resetToDefaults" />
          <Button label="Save Changes" icon="pi pi-check" @click="saveTheme" :loading="saving" />
        </FcFormActions>
      </template>
    </LoginThemeEditor>

    <p class="scope-note">
      This is the platform-wide theme. Individual clients can override it under
      <strong>Clients → (select a client) → Login Branding</strong>.
    </p>
  </div>
</template>

<style scoped>
.loading-container {
  display: flex;
  justify-content: center;
  padding: 60px;
}

.error-message {
  margin-bottom: 16px;
}

.scope-note {
  margin-top: 16px;
  font-size: 13px;
  color: #64748b;
}
</style>
