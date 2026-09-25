<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, watch } from "vue";
import { useRoute } from "vue-router";
import { oauthClientsApi, type OAuthClient } from "@/api/oauth-clients";
import { applicationsApi, type Application } from "@/api/applications";
import { clientsApi, type Client } from "@/api/clients";
import { portalAppsApi, type PortalApp } from "@/api/portal-apps";
import { getErrorMessage } from "@/utils/errors";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";
import { useDirtyForm } from "@/composables/useDirtyForm";

const emit = defineEmits<{
	changed: [];
}>();

const route = useRoute();

// Edit mode
const isEditing = ref(false);

const client = ref<OAuthClient | null>(null);
const applications = ref<Application[]>([]);
const clients = ref<Client[]>([]);
const loading = ref(true);
const saving = ref(false);
// `loadError` is the drawer-level (load) error — it replaces the whole body.
// Save failures use `saveError`, shown inline in the edit form, so a failed
// save never blanks the form the user is still editing.
const loadError = ref<string | null>(null);
const saveError = ref<string | null>(null);

const editForm = ref({
	clientName: "",
	redirectUris: [] as string[],
	postLogoutRedirectUris: [] as string[],
	allowedOrigins: [] as string[],
	grantTypes: [] as string[],
	defaultScopes: [] as string[],
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

const { dirty, markClean, reset: resetDirty } = useDirtyForm(() => ({
	...editForm.value,
}));

// No template ref on EntityDrawer: this drawer has no programmatic close
// (delete lives on the list rows) — all closes go through goToList().
const { id, goToList } = useDrawerRoute({
	listPath: "/authentication/oauth-clients",
	dirty: computed(() => isEditing.value && dirty.value),
});

// Secret rotation dialogs — local modals, so they stack above the drawer
const showRotateSecretDialog = ref(false);
const rotateLoading = ref(false);
const showNewSecretDialog = ref(false);
const newClientSecret = ref<string | null>(null);

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

// Redirect URIs are required for authorization_code or refresh_token grants
const requiresRedirectUri = computed(() => {
	return (
		editForm.value.grantTypes.includes("authorization_code") ||
		editForm.value.grantTypes.includes("refresh_token")
	);
});

const isValid = computed(() => {
	const hasClientName = editForm.value.clientName.trim() !== "";
	const hasGrantTypes = editForm.value.grantTypes.length > 0;
	const hasRedirectUris = editForm.value.redirectUris.length > 0;

	// Redirect URIs only required for authorization_code grant
	const redirectUriValid = !requiresRedirectUri.value || hasRedirectUris;

	return hasClientName && hasGrantTypes && redirectUriValid;
});

const validationErrors = computed(() => {
	const errors: string[] = [];
	if (editForm.value.clientName.trim() === "") {
		errors.push("Client name is required");
	}
	if (editForm.value.grantTypes.length === 0) {
		errors.push("At least one grant type is required");
	}
	if (requiresRedirectUri.value && editForm.value.redirectUris.length === 0) {
		errors.push(
			"At least one redirect URI is required for authorization_code or refresh_token grants",
		);
	}
	return errors;
});

// Reactive param: the drawer instance is reused when switching between rows.
watch(
	id,
	(value) => {
		if (value) void loadData(value);
	},
	{ immediate: true },
);

async function loadData(clientId: string) {
	loading.value = true;
	loadError.value = null;
	saveError.value = null;
	isEditing.value = false;
	resetDirty();
	newRedirectUri.value = "";
	newPostLogoutRedirectUri.value = "";
	newAllowedOrigin.value = "";
	showRotateSecretDialog.value = false;
	showNewSecretDialog.value = false;
	newClientSecret.value = null;
	try {
		// loadApplications never rejects — failures degrade to a warn toast
		const [clientData] = await Promise.all([
			oauthClientsApi.get(clientId),
			loadApplications(),
			loadClients(),
		]);
		client.value = clientData;
		resetEditForm();
		if (route.query["edit"] === "true") {
			startEditing();
		}
	} catch (e) {
		client.value = null;
		loadError.value =
			e instanceof Error ? e.message : "Failed to load OAuth client";
	} finally {
		loading.value = false;
	}
}

async function loadApplications() {
	try {
		// Only load user-facing applications (not integrations)
		const response = await applicationsApi.listApplicationsOnly(true);
		applications.value = response.applications || [];
	} catch (e: unknown) {
		console.error("Failed to load applications:", e);
		toast.warn("Warning", "Could not load applications: " + getErrorMessage(e, "Unknown error"));
	}
}

async function loadClients() {
	try {
		void loadPortalApps();
		const response = await clientsApi.list();
		clients.value = response.clients || [];
	} catch (e: unknown) {
		console.error("Failed to load clients:", e);
	}
}

function resetEditForm() {
	if (client.value) {
		editForm.value = {
			clientName: client.value.clientName || "",
			redirectUris: [...(client.value.redirectUris || [])],
			postLogoutRedirectUris: [...(client.value.postLogoutRedirectUris || [])],
			allowedOrigins: [...(client.value.allowedOrigins || [])],
			grantTypes: [...(client.value.grantTypes || [])],
			defaultScopes: [...(client.value.defaultScopes || [])],
			pkceRequired: client.value.pkceRequired ?? true,
			applicationIds: [...(client.value.applicationIds || [])],
			portalClientId: client.value.portalClientId || "",
			portalAppId: client.value.portalAppId || "",
			apiAccess: client.value.apiAccess ?? false,
		};
	}
}

function startEditing() {
	resetEditForm();
	saveError.value = null;
	isEditing.value = true;
	markClean();
}

function cancelEditing() {
	resetEditForm();
	saveError.value = null;
	isEditing.value = false;
	resetDirty();
}

function addRedirectUri() {
	const uri = newRedirectUri.value.trim();
	if (uri && !editForm.value.redirectUris.includes(uri)) {
		try {
			new URL(uri);
			editForm.value.redirectUris.push(uri);
			newRedirectUri.value = "";
		} catch {
		}
	}
}

function removeRedirectUri(uri: string) {
	editForm.value.redirectUris = editForm.value.redirectUris.filter(
		(u) => u !== uri,
	);
}

function addPostLogoutRedirectUri() {
	const uri = newPostLogoutRedirectUri.value.trim();
	if (uri && !editForm.value.postLogoutRedirectUris.includes(uri)) {
		try {
			new URL(uri);
			editForm.value.postLogoutRedirectUris.push(uri);
			newPostLogoutRedirectUri.value = "";
		} catch {
		}
	}
}

function removePostLogoutRedirectUri(uri: string) {
	editForm.value.postLogoutRedirectUris =
		editForm.value.postLogoutRedirectUris.filter((u) => u !== uri);
}

function addAllowedOrigin() {
	const origin = newAllowedOrigin.value.trim();
	if (origin && !editForm.value.allowedOrigins.includes(origin)) {
		try {
			const url = new URL(origin);
			if (url.pathname !== "/" && url.pathname !== "") {
				toast.error("Invalid Origin", "Origin should not include a path (e.g., https://example.com)");
				return;
			}
			editForm.value.allowedOrigins.push(url.origin);
			newAllowedOrigin.value = "";
		} catch {
		}
	}
}

function removeAllowedOrigin(origin: string) {
	editForm.value.allowedOrigins = editForm.value.allowedOrigins.filter(
		(o) => o !== origin,
	);
}

async function saveChanges() {
	if (!client.value || !isValid.value) return;

	saving.value = true;
	saveError.value = null;

	const targetId = client.value.id;
	try {
		await oauthClientsApi.update(
			targetId,
			{
				clientName: editForm.value.clientName.trim(),
				redirectUris: editForm.value.redirectUris,
				postLogoutRedirectUris: editForm.value.postLogoutRedirectUris,
				allowedOrigins: editForm.value.allowedOrigins,
				grantTypes: editForm.value.grantTypes,
				defaultScopes: editForm.value.defaultScopes,
				pkceRequired: editForm.value.pkceRequired,
				applicationIds: editForm.value.applicationIds,
				portalClientId: editForm.value.portalClientId ?? "",
				// Unlink when the portal flag is cleared or the app is cleared.
				portalAppId: editForm.value.portalClientId
					? (editForm.value.portalAppId ?? "")
					: "",
				apiAccess: editForm.value.apiAccess,
			},
			// Handled inline below — don't also fire the global red banner.
			{ suppressGlobalErrorToast: true },
		);

		client.value = await oauthClientsApi.get(targetId);
		resetEditForm();
		isEditing.value = false;
		resetDirty();
		toast.success("Success", "OAuth client updated successfully");
		emit("changed");
	} catch (e: unknown) {
		// Keep the form populated so the user can correct and retry.
		saveError.value = getErrorMessage(e, "Failed to update OAuth client");
	} finally {
		saving.value = false;
	}
}

async function rotateSecret() {
	if (!client.value) return;

	rotateLoading.value = true;

	try {
		const response = await oauthClientsApi.rotateSecret(client.value.id);
		// clientSecret is optional on the wire shape (PUBLIC clients have no
		// secret to rotate); normalise undefined → null for the dialog ref.
		newClientSecret.value = response.clientSecret ?? null;
		showRotateSecretDialog.value = false;
		showNewSecretDialog.value = true;
		toast.success("Success", "Client secret rotated successfully");
		emit("changed");
	} catch (e: unknown) {
	} finally {
		rotateLoading.value = false;
	}
}

function copySecret() {
	if (newClientSecret.value) {
		navigator.clipboard.writeText(newClientSecret.value);
		toast.success("Copied", "Client secret copied to clipboard");
	}
}

function copyClientId() {
	if (client.value) {
		navigator.clipboard.writeText(client.value.clientId);
		toast.success("Copied", "Client ID copied to clipboard");
	}
}

async function toggleActive() {
	if (!client.value) return;

	try {
		if (client.value.active) {
			await oauthClientsApi.deactivate(client.value.id);
			client.value.active = false;
			toast.success("Deactivated", "OAuth client has been deactivated");
		} else {
			await oauthClientsApi.activate(client.value.id);
			client.value.active = true;
			toast.success("Activated", "OAuth client has been activated");
		}
		emit("changed");
	} catch (e: unknown) {
	}
}

function formatDate(dateString: string) {
	return new Date(dateString).toLocaleString();
}

function getClientTypeSeverity(clientType: string) {
	return clientType === "PUBLIC" ? "info" : "warn";
}
</script>

<template>
  <EntityDrawer
    :title="client?.clientName || 'OAuth Client'"
    :subtitle="client?.clientId"
    size="wide"
    :loading="loading"
    :error="loadError"
    :dirty="isEditing && dirty"
    @close="goToList()"
  >
    <template v-if="client && !isEditing" #header-extra>
      <Tag :value="client.clientType" :severity="getClientTypeSeverity(client.clientType)" />
      <Tag
        :value="client.active ? 'Active' : 'Inactive'"
        :severity="client.active ? 'success' : 'secondary'"
      />
    </template>

    <template v-if="client">
      <!-- Configuration -->
      <FcFormSection title="Client Configuration" flat>
        <template v-if="!isEditing" #actions>
          <Button icon="pi pi-pencil" label="Edit" text @click="startEditing" />
        </template>

        <!-- View mode -->
        <div v-if="!isEditing" class="fc-detail-grid">
          <FcDetailField label="Client Name" :value="client.clientName" />
          <FcDetailField label="Client ID">
            <div class="client-id-row">
              <code class="client-id">{{ client.clientId }}</code>
              <Button
                icon="pi pi-copy"
                text
                size="small"
                v-tooltip="'Copy Client ID'"
                @click="copyClientId"
              />
            </div>
          </FcDetailField>

          <FcDetailField label="Redirect URIs" span>
            <div class="uri-list">
              <Chip v-for="uri in client.redirectUris" :key="uri" :label="uri" />
            </div>
          </FcDetailField>

          <FcDetailField label="Post-Logout Redirect URIs" span>
            <div
              v-if="client.postLogoutRedirectUris && client.postLogoutRedirectUris.length > 0"
              class="uri-list"
            >
              <Chip v-for="uri in client.postLogoutRedirectUris" :key="uri" :label="uri" />
            </div>
            <span v-else class="text-muted">No post-logout redirects configured</span>
          </FcDetailField>

          <FcDetailField label="Allowed CORS Origins" span>
            <div v-if="client.allowedOrigins && client.allowedOrigins.length > 0" class="uri-list">
              <Chip v-for="origin in client.allowedOrigins" :key="origin" :label="origin" />
            </div>
            <span v-else class="text-muted">No CORS origins configured</span>
          </FcDetailField>

          <FcDetailField label="Grant Types">
            <div class="tag-list">
              <Tag
                v-for="grant in client.grantTypes"
                :key="grant"
                :value="grant"
                severity="secondary"
              />
            </div>
          </FcDetailField>

          <FcDetailField label="Default Scopes">
            <div class="tag-list">
              <Tag
                v-for="scope in client.defaultScopes"
                :key="scope"
                :value="scope"
                severity="secondary"
              />
            </div>
          </FcDetailField>

          <FcDetailField label="PKCE Required">
            <i
              :class="client.pkceRequired ? 'pi pi-check text-success' : 'pi pi-times text-muted'"
            />
            {{ client.pkceRequired ? 'Yes' : 'No' }}
          </FcDetailField>

          <FcDetailField label="Associated Applications">
            <div v-if="(client.applicationIds?.length ?? 0) > 0" class="tag-list">
              <Tag
                v-for="appId in client.applicationIds"
                :key="appId"
                :value="applications.find(a => a.id === appId)?.name || appId"
                severity="info"
              />
            </div>
            <span v-else class="text-muted">No application restrictions</span>
          </FcDetailField>

          <FcDetailField label="Portal Owner Client">
            <span v-if="client.portalClientId">
              {{ clients.find(c => c.id === client?.portalClientId)?.name || client.portalClientId }}
            </span>
            <span v-else class="text-muted">Not a portal client</span>
          </FcDetailField>

          <FcDetailField v-if="client.portalClientId" label="Portal App">
            <span v-if="client.portalAppId">
              {{ portalApps.find(a => a.id === client?.portalAppId)?.name || client.portalAppId }}
              <code v-if="portalApps.find(a => a.id === client?.portalAppId)">
                ({{ portalApps.find(a => a.id === client?.portalAppId)?.code }})
              </code>
            </span>
            <span v-else class="text-muted">Legacy: whole client (no app gate)</span>
          </FcDetailField>

          <FcDetailField label="API Access">
            <Tag
              :value="client.apiAccess ? 'Authority-bearing tokens' : 'Identity tokens only'"
              :severity="client.apiAccess ? 'warn' : 'secondary'"
            />
          </FcDetailField>

          <FcDetailField label="Created" :value="formatDate(client.createdAt)" />
          <FcDetailField label="Last Updated" :value="formatDate(client.updatedAt)" />
        </div>

        <!-- Edit mode -->
        <div v-else class="edit-form">
          <FcFormField label="Client Name" required>
            <template #default="{ id: fieldId }">
              <InputText :id="fieldId" v-model="editForm.clientName" />
            </template>
          </FcFormField>

          <div class="field">
            <label>Redirect URIs *</label>
            <div class="redirect-uri-input">
              <InputText
                v-model="newRedirectUri"
                placeholder="https://app.example.com/callback"
                class="flex-grow"
                @keyup.enter="addRedirectUri"
              />
              <Button
                icon="pi pi-plus"
                @click="addRedirectUri"
                :disabled="!newRedirectUri.trim()"
              />
            </div>
            <div v-if="editForm.redirectUris.length > 0" class="uri-list">
              <Chip
                v-for="uri in editForm.redirectUris"
                :key="uri"
                :label="uri"
                removable
                @remove="removeRedirectUri(uri)"
              />
            </div>
            <small class="field-help">Must use HTTPS (except localhost).</small>
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
            <div
              v-if="editForm.postLogoutRedirectUris.length > 0"
              class="uri-list"
            >
              <Chip
                v-for="uri in editForm.postLogoutRedirectUris"
                :key="uri"
                :label="uri"
                removable
                @remove="removePostLogoutRedirectUri(uri)"
              />
            </div>
            <small class="field-help">
              OIDC RP-Initiated Logout. Required for session-end redirects — callers must also
              send id_token_hint.
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
            <div v-if="editForm.allowedOrigins.length > 0" class="uri-list">
              <Chip
                v-for="origin in editForm.allowedOrigins"
                :key="origin"
                :label="origin"
                removable
                @remove="removeAllowedOrigin(origin)"
              />
            </div>
            <small class="field-help"
              >Origins allowed to make browser requests to the token endpoint. Must use HTTPS
              (except localhost).</small
            >
          </div>

          <div class="field">
            <label for="grantTypes">Grant Types *</label>
            <MultiSelect
              id="grantTypes"
              v-model="editForm.grantTypes"
              :options="grantTypeOptions"
              optionLabel="label"
              optionValue="value"
              class="w-full"
            />
          </div>

          <div class="field">
            <label for="defaultScopes">Default Scopes</label>
            <MultiSelect
              id="defaultScopes"
              v-model="editForm.defaultScopes"
              :options="scopeOptions"
              optionLabel="label"
              optionValue="value"
              class="w-full"
            />
          </div>

          <div class="field checkbox-field">
            <Checkbox
              id="pkceRequired"
              v-model="editForm.pkceRequired"
              :binary="true"
              :disabled="client.clientType === 'PUBLIC'"
            />
            <label for="pkceRequired" class="checkbox-label">Require PKCE</label>
          </div>

          <div class="field">
            <label for="applications">Associated Applications</label>
            <MultiSelect
              id="applications"
              v-model="editForm.applicationIds"
              :options="applications"
              optionLabel="name"
              optionValue="id"
              placeholder="Select applications (optional)"
              class="w-full"
              filter
            />
            <small class="field-help">
              Only users with access to these applications can authenticate. Leave empty for no
              restrictions.
            </small>
          </div>

          <div class="field">
            <label for="portalClient">Portal owner client</label>
            <Select
              id="portalClient"
              v-model="editForm.portalClientId"
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
              client's ACTIVE portal users can authenticate through it. Clearing this removes the
              portal gate.
            </small>
          </div>

          <div v-if="editForm.portalClientId" class="field">
            <label for="portalAppEdit">Portal app</label>
            <Select
              id="portalAppEdit"
              v-model="editForm.portalAppId"
              :options="portalAppsFor(editForm.portalClientId)"
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
              id="apiAccessEdit"
              v-model="editForm.apiAccess"
              :binary="true"
              :disabled="!!editForm.portalClientId"
            />
            <label for="apiAccessEdit" class="checkbox-label">API access (trusted first-party)</label>
          </div>
          <small class="field-help">
            Interactive logins mint authority-bearing access tokens narrowed to the client's
            applications. Not available for portal clients.
          </small>

          <Message
            v-if="validationErrors.length > 0"
            severity="warn"
            :closable="false"
            class="validation-message"
          >
            <ul class="validation-list">
              <li v-for="err in validationErrors" :key="err">{{ err }}</li>
            </ul>
          </Message>

          <Message
            v-if="saveError"
            severity="error"
            :closable="false"
            class="validation-message"
          >
            {{ saveError }}
          </Message>
        </div>
      </FcFormSection>

      <!-- Client Secret -->
      <FcFormSection
        v-if="!isEditing && client.clientType === 'CONFIDENTIAL'"
        title="Client Secret"
        description="The client secret is encrypted and cannot be displayed. If you need a new secret, you can rotate it."
        flat
      >
        <Button
          label="Rotate Secret"
          icon="pi pi-refresh"
          severity="warn"
          @click="showRotateSecretDialog = true"
        />
      </FcFormSection>

      <!-- Actions -->
      <FcFormSection v-if="!isEditing" title="Actions" flat>
        <div class="action-items">
          <div class="action-item">
            <div class="action-info">
              <strong>{{ client.active ? 'Deactivate Client' : 'Activate Client' }}</strong>
              <p>
                {{
                  client.active
                    ? 'Inactive clients cannot be used to authenticate users.'
                    : 'Re-enable this client so applications can authenticate users with it.'
                }}
              </p>
            </div>
            <Button
              :label="client.active ? 'Deactivate' : 'Activate'"
              :icon="client.active ? 'pi pi-ban' : 'pi pi-check-circle'"
              :severity="client.active ? 'warn' : 'success'"
              outlined
              @click="toggleActive"
            />
          </div>
        </div>
      </FcFormSection>
    </template>

    <template v-if="isEditing" #footer>
      <FcFormActions :bordered="false">
        <Button v-if="dirty" label="Discard" text @click="cancelEditing" :disabled="saving" />
        <Button
          label="Save Changes"
          icon="pi pi-check"
          @click="saveChanges"
          :loading="saving"
          :disabled="!dirty || !isValid"
        />
      </FcFormActions>
    </template>
  </EntityDrawer>

  <!-- Rotate Secret Confirmation Dialog -->
  <Dialog
    v-model:visible="showRotateSecretDialog"
    header="Rotate Client Secret"
    modal
    :style="{ width: '450px' }"
  >
    <div class="dialog-content">
      <Message severity="warn" :closable="false">
        This will invalidate the current secret. Any applications using the old secret will stop
        working.
      </Message>
      <p>Are you sure you want to rotate the client secret?</p>
    </div>

    <template #footer>
      <Button
        label="Cancel"
        text
        @click="showRotateSecretDialog = false"
        :disabled="rotateLoading"
      />
      <Button
        label="Rotate Secret"
        icon="pi pi-refresh"
        severity="warn"
        @click="rotateSecret"
        :loading="rotateLoading"
      />
    </template>
  </Dialog>

  <!-- New Secret Display Dialog (one-time reveal — stays a local modal) -->
  <Dialog
    v-model:visible="showNewSecretDialog"
    header="New Client Secret"
    modal
    :closable="false"
    :style="{ width: '500px' }"
  >
    <div class="dialog-content">
      <Message severity="warn" :closable="false">
        Copy this secret now. It will not be shown again.
      </Message>

      <div class="secret-display">
        <code class="secret-code">{{ newClientSecret }}</code>
        <Button icon="pi pi-copy" text v-tooltip="'Copy to clipboard'" @click="copySecret" />
      </div>
    </div>

    <template #footer>
      <Button
        label="I've copied the secret"
        icon="pi pi-check"
        @click="showNewSecretDialog = false"
      />
    </template>
  </Dialog>
</template>

<style scoped>
.client-id-row {
  display: flex;
  align-items: center;
  gap: 4px;
}

.client-id {
  background: #f1f5f9;
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 13px;
  word-break: break-all;
}

.edit-form {
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

.uri-list {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.tag-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.redirect-uri-input {
  display: flex;
  gap: 8px;
}

.flex-grow {
  flex: 1;
}

.action-items {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.action-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  padding: 16px;
  background: #fafafa;
  border-radius: 8px;
  border: 1px solid #e5e7eb;
}

.action-info strong {
  display: block;
  margin-bottom: 4px;
}

.action-info p {
  margin: 0;
  font-size: 13px;
  color: #64748b;
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

.text-muted {
  color: #94a3b8;
}

.text-success {
  color: #22c55e;
}

.w-full {
  width: 100%;
}

.validation-message {
  margin-bottom: 0;
}

.validation-list {
  margin: 0;
  padding-left: 20px;
}

.validation-list li {
  margin: 2px 0;
}
</style>
