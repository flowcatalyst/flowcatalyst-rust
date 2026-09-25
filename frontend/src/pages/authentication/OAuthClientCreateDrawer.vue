<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, onMounted } from "vue";
import { oauthClientsApi, type ClientType } from "@/api/oauth-clients";
import { applicationsApi, type Application } from "@/api/applications";
import { clientsApi, type Client } from "@/api/clients";
import { portalAppsApi, type PortalApp } from "@/api/portal-apps";
import { getErrorMessage } from "@/utils/errors";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";

const emit = defineEmits<{
	changed: [];
}>();

const applications = ref<Application[]>([]);
const clients = ref<Client[]>([]);
const submitting = ref(false);
const error = ref<string | null>(null);

// Form state
const form = ref({
	clientName: "",
	clientType: "PUBLIC" as ClientType,
	redirectUris: [] as string[],
	postLogoutRedirectUris: [] as string[],
	allowedOrigins: [] as string[],
	grantTypes: ["authorization_code", "refresh_token"],
	defaultScopes: ["openid", "profile", "email"],
	pkceRequired: true,
	applicationIds: [] as string[],
	portalClientId: "" as string,
	portalAppId: "" as string,
	apiAccess: false,
});

// Portal apps, for the "Portal app" picker (scoped to the chosen owner client).
const portalApps = ref<PortalApp[]>([]);
function portalAppsFor(clientId: string) {
	return portalApps.value.filter((a) => a.clientId === clientId);
}
async function loadPortalApps() {
	try {
		portalApps.value = (await portalAppsApi.list()).portalApps;
	} catch {
		// Optional enrichment — the picker just stays empty.
	}
}


const newRedirectUri = ref("");
const newPostLogoutRedirectUri = ref("");
const newAllowedOrigin = ref("");

// Secret dialog state — for CONFIDENTIAL clients the one-time secret must be
// acknowledged BEFORE navigating away from this drawer.
const showSecretDialog = ref(false);
const clientSecret = ref<string | null>(null);
const createdClientId = ref<string | null>(null);
const createdOAuthClientId = ref<string | null>(null);
const created = ref(false);

// Cheap dirty check: anything typed or selected counts. Grant types and
// scopes are excluded (they have non-empty defaults). Cleared once the
// client exists so the post-create navigation doesn't prompt to discard.
const dirty = computed(
	() =>
		!created.value &&
		(form.value.clientName.trim() !== "" ||
			form.value.redirectUris.length > 0 ||
			form.value.postLogoutRedirectUris.length > 0 ||
			form.value.allowedOrigins.length > 0 ||
			form.value.applicationIds.length > 0),
);

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { goToList, replaceToDetail } = useDrawerRoute({
	listPath: "/authentication/oauth-clients",
	dirty,
});

const clientTypeOptions = [
	{
		label: "Public (SPA, Mobile)",
		value: "PUBLIC",
		description: "No client secret, PKCE required",
	},
	{
		label: "Confidential (Server)",
		value: "CONFIDENTIAL",
		description: "Has client secret",
	},
];

const grantTypeOptions = [
	{ label: "Authorization Code", value: "authorization_code" },
	{ label: "Refresh Token", value: "refresh_token" },
	{ label: "Client Credentials", value: "client_credentials" },
];

const scopeOptions = [
	{ label: "openid", value: "openid" },
	{ label: "profile", value: "profile" },
	{ label: "email", value: "email" },
	{ label: "offline_access", value: "offline_access" },
];

const isValid = computed(() => {
	return (
		form.value.clientName.trim() !== "" &&
		form.value.redirectUris.length > 0 &&
		form.value.grantTypes.length > 0
	);
});

onMounted(async () => {
	await loadApplications();
	await loadClients();
});

async function loadClients() {
	try {
		void loadPortalApps();
		const response = await clientsApi.list();
		clients.value = response.clients || [];
	} catch (e: unknown) {
		console.error("Failed to load clients:", e);
	}
}

async function loadApplications() {
	try {
		// Only load user-facing applications (not integrations)
		// OAuth clients are associated with applications, not integrations
		const response = await applicationsApi.listApplicationsOnly(true);
		applications.value = response.applications || [];
	} catch (e: unknown) {
		console.error("Failed to load applications:", e);
		toast.warn("Warning", "Could not load applications: " + getErrorMessage(e, "Unknown error"));
	}
}

function addRedirectUri() {
	const uri = newRedirectUri.value.trim();
	if (uri && !form.value.redirectUris.includes(uri)) {
		// Basic URL validation
		try {
			new URL(uri);
			form.value.redirectUris.push(uri);
			newRedirectUri.value = "";
		} catch {
		}
	}
}

function removeRedirectUri(uri: string) {
	form.value.redirectUris = form.value.redirectUris.filter((u) => u !== uri);
}

function addPostLogoutRedirectUri() {
	const uri = newPostLogoutRedirectUri.value.trim();
	if (uri && !form.value.postLogoutRedirectUris.includes(uri)) {
		try {
			new URL(uri);
			form.value.postLogoutRedirectUris.push(uri);
			newPostLogoutRedirectUri.value = "";
		} catch {
		}
	}
}

function removePostLogoutRedirectUri(uri: string) {
	form.value.postLogoutRedirectUris = form.value.postLogoutRedirectUris.filter(
		(u) => u !== uri,
	);
}

function addAllowedOrigin() {
	const origin = newAllowedOrigin.value.trim();
	if (origin && !form.value.allowedOrigins.includes(origin)) {
		// Basic URL validation - must be a valid origin (scheme + host)
		try {
			const url = new URL(origin);
			// Origin should not have path (other than /)
			if (url.pathname !== "/" && url.pathname !== "") {
				toast.error("Invalid Origin", "Origin should not include a path (e.g., https://example.com)");
				return;
			}
			// Use the origin (scheme + host + port)
			form.value.allowedOrigins.push(url.origin);
			newAllowedOrigin.value = "";
		} catch {
		}
	}
}

function removeAllowedOrigin(origin: string) {
	form.value.allowedOrigins = form.value.allowedOrigins.filter(
		(o) => o !== origin,
	);
}

async function createClient() {
	if (!isValid.value) return;

	submitting.value = true;
	error.value = null;

	try {
		const response = await oauthClientsApi.create({
			clientName: form.value.clientName.trim(),
			clientType: form.value.clientType,
			redirectUris: form.value.redirectUris,
			postLogoutRedirectUris:
				form.value.postLogoutRedirectUris.length > 0
					? form.value.postLogoutRedirectUris
					: undefined,
			allowedOrigins:
				form.value.allowedOrigins.length > 0
					? form.value.allowedOrigins
					: undefined,
			grantTypes: form.value.grantTypes,
			defaultScopes:
				form.value.defaultScopes.length > 0
					? form.value.defaultScopes
					: undefined,
			pkceRequired: form.value.pkceRequired,
			applicationIds:
				form.value.applicationIds.length > 0
					? form.value.applicationIds
					: undefined,
			portalClientId: form.value.portalClientId || undefined,
			portalAppId:
				(form.value.portalClientId && form.value.portalAppId) || undefined,
			apiAccess: form.value.apiAccess || undefined,
		});

		toast.success("Success", `OAuth client "${response.client.clientName}" created successfully`);
		emit("changed");
		created.value = true;

		if (response.clientSecret) {
			// CONFIDENTIAL: show the one-time secret dialog before navigating
			// away — the modal stacks above the still-open drawer.
			createdClientId.value = response.client.id;
			createdOAuthClientId.value = response.client.clientId;
			clientSecret.value = response.clientSecret;
			showSecretDialog.value = true;
		} else {
			// PUBLIC: no secret to acknowledge — hand off to the detail drawer.
			replaceToDetail(response.client.id);
		}
	} catch (e: unknown) {
		error.value = getErrorMessage(e, "Failed to create OAuth client");
	} finally {
		submitting.value = false;
	}
}

function copyCreatedClientId() {
	if (createdOAuthClientId.value) {
		void navigator.clipboard.writeText(createdOAuthClientId.value);
		toast.info("Copied", "Client ID copied to clipboard");
	}
}

function copySecret() {
	if (clientSecret.value) {
		navigator.clipboard.writeText(clientSecret.value);
		toast.success("Copied", "Client secret copied to clipboard");
	}
}

function closeSecretDialog() {
	showSecretDialog.value = false;
	if (createdClientId.value) {
		replaceToDetail(createdClientId.value);
	} else {
		goToList();
	}
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    title="Create OAuth Client"
    subtitle="Register a new OAuth2/OIDC client for applications that use FlowCatalyst as their identity provider"
    :dirty="dirty"
    @close="goToList()"
  >
    <Message
      v-if="error"
      severity="error"
      class="error-message"
      :closable="true"
      @close="error = null"
    >
      {{ error }}
    </Message>

    <div class="form-content">
      <div class="field">
        <label for="clientName">Client Name *</label>
        <InputText
          id="clientName"
          v-model="form.clientName"
          placeholder="e.g., Production SPA, Development Server"
          class="w-full"
        />
        <small class="field-help">A human-readable name for this client</small>
      </div>

      <div class="field">
        <label for="clientType">Client Type *</label>
        <Select
          id="clientType"
          v-model="form.clientType"
          :options="clientTypeOptions"
          optionLabel="label"
          optionValue="value"
          class="w-full"
        >
          <template #option="slotProps">
            <div class="type-option">
              <span class="type-label">{{ slotProps.option.label }}</span>
              <span class="type-description">{{ slotProps.option.description }}</span>
            </div>
          </template>
        </Select>
      </div>

      <div class="field">
        <label>Redirect URIs *</label>
        <div class="redirect-uri-input">
          <InputText
            v-model="newRedirectUri"
            placeholder="https://app.example.com/callback"
            class="flex-grow"
            @keyup.enter="addRedirectUri"
          />
          <Button icon="pi pi-plus" @click="addRedirectUri" :disabled="!newRedirectUri.trim()" />
        </div>
        <div v-if="form.redirectUris.length > 0" class="uri-list">
          <Chip
            v-for="uri in form.redirectUris"
            :key="uri"
            :label="uri"
            removable
            @remove="removeRedirectUri(uri)"
          />
        </div>
        <small class="field-help"
          >Allowed callback URLs for OAuth redirects. Must use HTTPS (except localhost).</small
        >
      </div>

      <div class="field">
        <label>Post-Logout Redirect URIs</label>
        <div class="redirect-uri-input">
          <InputText
            v-model="newPostLogoutRedirectUri"
            placeholder="https://app.example.com/logged-out"
            class="flex-grow"
            @keyup.enter="addPostLogoutRedirectUri"
          />
          <Button
            icon="pi pi-plus"
            @click="addPostLogoutRedirectUri"
            :disabled="!newPostLogoutRedirectUri.trim()"
          />
        </div>
        <div v-if="form.postLogoutRedirectUris.length > 0" class="uri-list">
          <Chip
            v-for="uri in form.postLogoutRedirectUris"
            :key="uri"
            :label="uri"
            removable
            @remove="removePostLogoutRedirectUri(uri)"
          />
        </div>
        <small class="field-help">
          Allowed URLs for OIDC RP-Initiated Logout (post_logout_redirect_uri). Required for
          session-end redirects — callers must also send id_token_hint.
        </small>
      </div>

      <div class="field">
        <label>Allowed CORS Origins</label>
        <div class="redirect-uri-input">
          <InputText
            v-model="newAllowedOrigin"
            placeholder="https://app.example.com"
            class="flex-grow"
            @keyup.enter="addAllowedOrigin"
          />
          <Button
            icon="pi pi-plus"
            @click="addAllowedOrigin"
            :disabled="!newAllowedOrigin.trim()"
          />
        </div>
        <div v-if="form.allowedOrigins.length > 0" class="uri-list">
          <Chip
            v-for="origin in form.allowedOrigins"
            :key="origin"
            :label="origin"
            removable
            @remove="removeAllowedOrigin(origin)"
          />
        </div>
        <small class="field-help"
          >Origins allowed to make browser requests to the token endpoint. Must use HTTPS (except
          localhost).</small
        >
      </div>

      <div class="field">
        <label for="grantTypes">Grant Types *</label>
        <MultiSelect
          id="grantTypes"
          v-model="form.grantTypes"
          :options="grantTypeOptions"
          optionLabel="label"
          optionValue="value"
          placeholder="Select grant types"
          class="w-full"
        />
      </div>

      <div class="field">
        <label for="defaultScopes">Default Scopes</label>
        <MultiSelect
          id="defaultScopes"
          v-model="form.defaultScopes"
          :options="scopeOptions"
          optionLabel="label"
          optionValue="value"
          placeholder="Select scopes"
          class="w-full"
        />
      </div>

      <div class="field checkbox-field">
        <Checkbox
          id="pkceRequired"
          v-model="form.pkceRequired"
          :binary="true"
          :disabled="form.clientType === 'PUBLIC'"
        />
        <label for="pkceRequired" class="checkbox-label">Require PKCE</label>
      </div>
      <small v-if="form.clientType === 'PUBLIC'" class="field-help">
        PKCE is always required for public clients
      </small>

      <div class="field">
        <label for="applications">Associated Applications</label>
        <MultiSelect
          id="applications"
          v-model="form.applicationIds"
          :options="applications"
          optionLabel="name"
          optionValue="id"
          placeholder="Select applications (optional)"
          class="w-full"
          filter
        >
          <template #option="slotProps">
            <div class="app-option">
              <span class="app-name">{{ slotProps.option.name }}</span>
              <span class="app-code">{{ slotProps.option.code }}</span>
            </div>
          </template>
        </MultiSelect>
        <small class="field-help">
          Only users with access to these applications can authenticate. Leave empty for no
          restrictions.
        </small>
      </div>

      <div class="field">
        <label for="portalClient">Portal owner client</label>
        <Select
          id="portalClient"
          v-model="form.portalClientId"
          :options="clients"
          optionLabel="name"
          optionValue="id"
          placeholder="Not a portal client"
          showClear
          filter
          class="w-full"
        />
        <small class="field-help">
          Marks this OAuth client as a portal entry point for the selected client: only that
          client's ACTIVE portal users can authenticate through it.
        </small>
      </div>

      <div v-if="form.portalClientId" class="field">
        <label for="portalApp">Portal app</label>
        <Select
          id="portalApp"
          v-model="form.portalAppId"
          :options="portalAppsFor(form.portalClientId)"
          optionLabel="name"
          optionValue="id"
          placeholder="Legacy: whole client (no app)"
          showClear
          class="w-full"
        >
          <template #option="{ option }">{{ option.name }} <code>({{ option.code }})</code></template>
        </Select>
        <small class="field-help">
          Which of the client's portals this OAuth client fronts. Users need a grant for the
          app to sign in, and the id_token carries its <code>portal_app_code</code>. Manage apps
          under Portal → Portal Apps.
        </small>
      </div>

      <div class="field checkbox-field">
        <Checkbox
          id="apiAccess"
          v-model="form.apiAccess"
          :binary="true"
          :disabled="!!form.portalClientId"
        />
        <label for="apiAccess" class="checkbox-label">API access (trusted first-party)</label>
      </div>
      <small class="field-help">
        Interactive logins through this client mint authority-bearing access tokens, narrowed
        to the client's associated applications. Leave off for third-party apps — they should
        authenticate users from the id_token only. Not available for portal clients.
      </small>
    </div>

    <template #footer>
      <FcFormActions :bordered="false">
        <Button label="Cancel" text @click="drawer?.close()" :disabled="submitting" />
        <Button
          label="Create OAuth Client"
          icon="pi pi-plus"
          @click="createClient"
          :loading="submitting"
          :disabled="!isValid"
        />
      </FcFormActions>
    </template>
  </EntityDrawer>

  <!-- Client Secret Dialog (one-time reveal — modal stacks above the drawer) -->
  <Dialog
    v-model:visible="showSecretDialog"
    header="Client Secret Generated"
    modal
    :closable="false"
    :style="{ width: '500px' }"
  >
    <div class="dialog-content">
      <Message severity="warn" :closable="false">
        Copy this secret now. It will not be shown again.
      </Message>

      <label class="secret-label">Client ID</label>
      <div class="secret-display">
        <code class="secret-code">{{ createdOAuthClientId }}</code>
        <Button icon="pi pi-copy" text v-tooltip="'Copy Client ID'" @click="copyCreatedClientId" />
      </div>

      <label class="secret-label">Client Secret (shown once)</label>
      <div class="secret-display">
        <code class="secret-code">{{ clientSecret }}</code>
        <Button icon="pi pi-copy" text v-tooltip="'Copy to clipboard'" @click="copySecret" />
      </div>
    </div>

    <template #footer>
      <Button label="I've copied the secret" icon="pi pi-check" @click="closeSecretDialog" />
    </template>
  </Dialog>
</template>

<style scoped>
.secret-label {
  display: block;
  font-weight: 600;
  font-size: 0.85rem;
  margin: 0.75rem 0 0.25rem;
}
.error-message {
  margin-bottom: 16px;
}

.form-content {
  display: flex;
  flex-direction: column;
  gap: 20px;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.field label {
  font-weight: 500;
  color: #334155;
}

.field-help {
  color: #64748b;
  font-size: 12px;
}

.checkbox-field {
  flex-direction: row;
  align-items: center;
  gap: 8px;
}

.checkbox-label {
  margin: 0;
  cursor: pointer;
}

.type-option {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 4px 0;
}

.type-option .type-label {
  font-size: 14px;
  font-weight: 500;
}

.type-option .type-description {
  font-size: 12px;
  color: #64748b;
}

.redirect-uri-input {
  display: flex;
  gap: 8px;
}

.flex-grow {
  flex: 1;
}

.uri-list {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 8px;
}

.app-option {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 4px 0;
}

.app-option .app-name {
  font-size: 14px;
  font-weight: 500;
}

.app-option .app-code {
  font-size: 12px;
  color: #64748b;
  font-family: monospace;
}

.dialog-content {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.secret-display {
  display: flex;
  align-items: center;
  gap: 8px;
  background: #f8fafc;
  padding: 12px;
  border-radius: 6px;
  border: 1px solid #e2e8f0;
}

.secret-code {
  flex: 1;
  font-size: 13px;
  word-break: break-all;
  color: #1e293b;
}

.w-full {
  width: 100%;
}
</style>
