<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import { clientsApi, type Client } from "@/api/clients";
import { configApi, defaultLoginTheme, type LoginTheme } from "@/api/config";
import { getErrorMessage } from "@/utils/errors";
import { useReturnTo } from "@/composables/useReturnTo";
import LoginThemeEditor from "@/components/theme/LoginThemeEditor.vue";

const route = useRoute();
const { returnTo } = useReturnTo();

const clientId = route.params["id"] as string;

const loading = ref(true);
const client = ref<Client | null>(null);
const loadError = ref<string | null>(null);

// `themeEnabled` reflects whether a CLIENT-scoped theme row exists: off
// means this client's users see the platform theme, and saving with it off
// deletes the override rather than storing a copy.
const theme = ref<LoginTheme>(defaultLoginTheme());
const themeEnabled = ref(false);
const savingTheme = ref(false);

// The URL a relying party sends users to in order to get this branding.
// The client hint is optional and cosmetic — without it the sign-in page
// falls back to the platform theme.
const brandedLoginHint = computed(() =>
	client.value ? `/oauth/authorize?…&client=${client.value.identifier}` : "",
);

onMounted(async () => {
	loading.value = true;
	await Promise.all([loadClient(), loadTheme()]);
	loading.value = false;
});

async function loadClient() {
	try {
		client.value = await clientsApi.get(clientId);
	} catch {
		client.value = null;
		loadError.value = "Client not found";
	}
}

async function loadTheme() {
	try {
		// The platform theme doubles as the starting point for a client that
		// has no override yet, so the admin customises from the real look
		// rather than from hard-coded defaults.
		const [globalJson, clientJson] = await Promise.all([
			configApi.getLoginThemeConfig(),
			configApi.getLoginThemeConfig(clientId),
		]);

		let base = defaultLoginTheme();
		if (globalJson) base = { ...base, ...safeParse(globalJson) };

		themeEnabled.value = Boolean(clientJson);
		theme.value = clientJson ? { ...base, ...safeParse(clientJson) } : base;
	} catch (error) {
		console.error("Failed to load client theme:", error);
	}
}

function safeParse(json: string): Partial<LoginTheme> {
	try {
		return JSON.parse(json) as Partial<LoginTheme>;
	} catch {
		return {};
	}
}

async function saveTheme() {
	savingTheme.value = true;
	try {
		if (themeEnabled.value) {
			await configApi.setLoginThemeConfig(theme.value, clientId);
			toast.success("Success", "Login branding saved");
		} else {
			await configApi.clearLoginThemeConfig(clientId);
			toast.success("Success", "Login branding reset to the platform theme");
		}
		await loadTheme();
	} catch (e) {
		toast.error("Error", getErrorMessage(e, "Failed to save login branding"));
	} finally {
		savingTheme.value = false;
	}
}

function goBack() {
	returnTo(`/clients/${clientId}`);
}
</script>

<template>
  <div class="page-container">
    <div v-if="loading" class="loading-container">
      <ProgressSpinner strokeWidth="3" />
    </div>

    <template v-else>
      <header class="page-header">
        <Button
          icon="pi pi-arrow-left"
          text
          severity="secondary"
          @click="goBack"
          v-tooltip="'Back'"
        />
        <div>
          <h1 class="page-title">Login Branding</h1>
          <p class="page-subtitle" v-if="client">
            Custom sign-in appearance for <strong>{{ client.name }}</strong>
          </p>
        </div>
      </header>

      <Message v-if="loadError" severity="error" class="error-message">{{ loadError }}</Message>

      <div v-else class="fc-form">
        <FcFormSection title="Login Branding">
          <div class="theme-toggle">
            <ToggleSwitch v-model="themeEnabled" input-id="client-theme-enabled" />
            <label for="client-theme-enabled">
              <strong>Use custom branding for this client</strong>
              <p>
                When off, this client's users see the platform-wide theme. Turning it on starts
                from the current platform theme so you only change what differs.
              </p>
            </label>
          </div>

          <LoginThemeEditor v-if="themeEnabled" v-model="theme" class="theme-editor-slot" />

          <FcFormActions>
            <Button
              label="Save Changes"
              icon="pi pi-check"
              :loading="savingTheme"
              @click="saveTheme"
            />
          </FcFormActions>

          <p class="help-text">
            Applies to the sign-in, forgot-password and reset-password pages. To get this
            branding, the application sends users to
            <code>{{ brandedLoginHint }}</code
            >. Portal users are not affected.
          </p>
        </FcFormSection>
      </div>
    </template>
  </div>
</template>

<style scoped>
.page-header {
  gap: 16px;
}

.loading-container {
  display: flex;
  justify-content: center;
  padding: 60px;
}

.error-message {
  margin-bottom: 16px;
}

.theme-toggle {
  display: flex;
  align-items: flex-start;
  gap: 12px;
}

.theme-toggle label {
  cursor: pointer;
}

.theme-toggle p {
  margin: 4px 0 0;
  font-size: 13px;
  color: #64748b;
}

.theme-editor-slot {
  margin-top: 20px;
}

.help-text {
  margin-top: 12px;
  font-size: 13px;
  color: #64748b;
}

.help-text code {
  font-size: 12px;
  word-break: break-all;
}
</style>
