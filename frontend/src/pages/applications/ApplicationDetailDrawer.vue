<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { computed, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { useConfirm } from "primevue/useconfirm";
import {
	applicationsApi,
	type Application,
	type ServiceAccountCredentials,
	type LoginClientCredentials,
} from "@/api/applications";
import { docsApi, type DocSummary } from "@/api/docs";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";
import { useDirtyForm } from "@/composables/useDirtyForm";

const emit = defineEmits<{
	changed: [];
}>();

const route = useRoute();
const confirm = useConfirm();

const editing = ref(false);

// Edit form
const editName = ref("");
const editDescription = ref("");
const editDefaultBaseUrl = ref("");
const editIconUrl = ref("");
const editWebsite = ref("");
const editLogo = ref("");
const editLogoMimeType = ref("");

const { dirty, markClean, reset: resetDirty } = useDirtyForm(() => ({
	name: editName.value,
	description: editDescription.value,
	defaultBaseUrl: editDefaultBaseUrl.value,
	iconUrl: editIconUrl.value,
	website: editWebsite.value,
	logo: editLogo.value,
	logoMimeType: editLogoMimeType.value,
}));

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { id, goToList } = useDrawerRoute({
	listPath: "/applications",
	dirty: computed(() => editing.value && dirty.value),
});

const loading = ref(true);
const loadError = ref<string | null>(null);
const application = ref<Application | null>(null);
const saving = ref(false);

// Service account provisioning
const provisioning = ref(false);
const showCredentialsDialog = ref(false);
const provisionedCredentials = ref<ServiceAccountCredentials | null>(null);

// Login client provisioning
const provisioningLoginClient = ref(false);
const showLoginClientDialog = ref(false);
const provisionedLoginClient = ref<LoginClientCredentials | null>(null);
const loginClientType = ref<"PUBLIC" | "CONFIDENTIAL">("PUBLIC");
const loginClientRedirectUris = ref<string[]>([]);
const newLoginRedirectUri = ref("");

// Reactive param: the drawer instance is reused when switching between rows.
watch(
	id,
	async (value) => {
		if (!value) return;
		resetTransientState();
		await loadApplication(value);
		if (route.query["edit"] === "true") {
			startEditing();
		}
	},
	{ immediate: true },
);

/** Row-switch hygiene: clear per-application UI state (edit mode, open
 * credential dialogs, the login-client provisioning form) before loading the
 * next application into the reused drawer instance. */
function resetTransientState() {
	editing.value = false;
	resetDirty();
	showCredentialsDialog.value = false;
	provisionedCredentials.value = null;
	showLoginClientDialog.value = false;
	provisionedLoginClient.value = null;
	loginClientRedirectUris.value = [];
	newLoginRedirectUri.value = "";
	loginClientType.value = "PUBLIC";
	application.value = null;
	loadError.value = null;
}

async function loadApplication(applicationId: string) {
	loading.value = true;
	loadError.value = null;
	try {
		application.value = await applicationsApi.get(applicationId);
		void loadAppDocs();
	} catch {
		application.value = null;
		loadError.value = "Application not found";
	} finally {
		loading.value = false;
	}
}

// Documentation the application synced (POST /api/applications/{code}/docs/sync),
// shown here for its administrators and rendered in Platform → Documentation.
const appDocs = ref<DocSummary[]>([]);

async function loadAppDocs() {
	appDocs.value = [];
	const code = application.value?.code;
	if (!code) return;
	try {
		const index = await docsApi.list();
		appDocs.value =
			index.applications.find((g) => g.applicationCode === code)?.docs ?? [];
	} catch {
		// Non-fatal: the drawer works without the docs section.
	}
}

function startEditing() {
	if (application.value) {
		editName.value = application.value.name;
		editDescription.value = application.value.description || "";
		editDefaultBaseUrl.value = application.value.defaultBaseUrl || "";
		editIconUrl.value = application.value.iconUrl || "";
		editWebsite.value = application.value.website || "";
		editLogo.value = application.value.logo || "";
		editLogoMimeType.value = application.value.logoMimeType || "";
		editing.value = true;
		markClean();
	}
}

function cancelEditing() {
	editing.value = false;
	resetDirty();
}

async function saveChanges() {
	const appId = application.value?.id;
	if (!appId) return;

	saving.value = true;
	try {
		await applicationsApi.update(appId, {
			name: editName.value,
			description: editDescription.value || undefined,
			defaultBaseUrl: editDefaultBaseUrl.value || undefined,
			iconUrl: editIconUrl.value || undefined,
			website: editWebsite.value || undefined,
			logo: editLogo.value || undefined,
			logoMimeType: editLogoMimeType.value || undefined,
		});
		await loadApplication(appId);
		editing.value = false;
		resetDirty();
		toast.success("Success", "Application updated");
		emit("changed");
	} catch {
		// update errors surface via the global error toast
	} finally {
		saving.value = false;
	}
}

function confirmActivate() {
	confirm.require({
		message: "Activate this application?",
		header: "Activate Application",
		icon: "pi pi-check-circle",
		acceptLabel: "Activate",
		accept: activateApplication,
	});
}

async function activateApplication() {
	const appId = application.value?.id;
	if (!appId) return;
	try {
		application.value = await applicationsApi.activate(appId);
		toast.success("Success", "Application activated");
		emit("changed");
	} catch {
		// errors surface via the global error toast
	}
}

function confirmDeactivate() {
	confirm.require({
		message:
			"Deactivate this application? It will no longer be available for new event types.",
		header: "Deactivate Application",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Deactivate",
		acceptClass: "p-button-warning",
		accept: deactivateApplication,
	});
}

async function deactivateApplication() {
	const appId = application.value?.id;
	if (!appId) return;
	try {
		application.value = await applicationsApi.deactivate(appId);
		toast.success("Success", "Application deactivated");
		emit("changed");
	} catch {
		// errors surface via the global error toast
	}
}

function confirmDelete() {
	confirm.require({
		message: "Delete this application? This cannot be undone.",
		header: "Delete Application",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Delete",
		acceptClass: "p-button-danger",
		accept: deleteApplication,
	});
}

async function deleteApplication() {
	const appId = application.value?.id;
	if (!appId) return;
	try {
		await applicationsApi.delete(appId);
		toast.success("Success", "Application deleted");
		emit("changed");
		editing.value = false;
		void drawer.value?.close(true);
	} catch {
		// errors surface via the global error toast
	}
}

async function provisionServiceAccount() {
	const appId = application.value?.id;
	if (!appId) return;

	provisioning.value = true;
	try {
		const result = await applicationsApi.provisionServiceAccount(appId);
		provisionedCredentials.value = result.serviceAccount;
		showCredentialsDialog.value = true;

		// Reload application to get updated serviceAccountId
		await loadApplication(appId);
		emit("changed");
	} catch {
		// errors surface via the global error toast
	} finally {
		provisioning.value = false;
	}
}

function onCredentialsDialogClose() {
	showCredentialsDialog.value = false;
	provisionedCredentials.value = null;
}

function addLoginRedirectUri() {
	const uri = newLoginRedirectUri.value.trim();
	if (uri && !loginClientRedirectUris.value.includes(uri)) {
		loginClientRedirectUris.value.push(uri);
		newLoginRedirectUri.value = "";
	}
}

function removeLoginRedirectUri(uri: string) {
	loginClientRedirectUris.value = loginClientRedirectUris.value.filter(
		(u) => u !== uri,
	);
}

async function provisionLoginClient() {
	const appId = application.value?.id;
	if (!appId) return;
	if (loginClientRedirectUris.value.length === 0) {
		toast.error("Validation", "At least one redirect URI is required");
		return;
	}

	provisioningLoginClient.value = true;
	try {
		const result = await applicationsApi.provisionLoginClient(appId, {
			clientType: loginClientType.value,
			redirectUris: loginClientRedirectUris.value,
		});
		provisionedLoginClient.value = result.loginClient;
		showLoginClientDialog.value = true;

		// Reload application so `hasLoginClient` flips to true and the
		// form is replaced by the "Provisioned" status.
		await loadApplication(appId);
		emit("changed");
	} catch {
		// errors surface via the global error toast
	} finally {
		provisioningLoginClient.value = false;
	}
}

function onLoginClientDialogClose() {
	showLoginClientDialog.value = false;
	provisionedLoginClient.value = null;
	// Reset the form for the next provisioning round (covers the case where
	// the user deletes the client later and wants to re-provision).
	loginClientRedirectUris.value = [];
	newLoginRedirectUri.value = "";
	loginClientType.value = "PUBLIC";
}

function copyToClipboard(text: string) {
	navigator.clipboard.writeText(text);
	toast.info("Copied", "Copied to clipboard");
}

function formatDate(dateString: string) {
	return new Date(dateString).toLocaleString();
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    :title="application?.name || 'Application'"
    :subtitle="application?.code"
    size="wide"
    :loading="loading"
    :error="loadError"
    :dirty="editing && dirty"
    @close="goToList()"
  >
    <template v-if="application" #header-extra>
      <Tag
        :value="application.active ? 'Active' : 'Inactive'"
        :severity="application.active ? 'success' : 'secondary'"
      />
    </template>

    <template v-if="application">
      <!-- Details -->
      <FcFormSection title="Application Details" flat>
        <template v-if="!editing" #actions>
          <Button icon="pi pi-pencil" label="Edit" text @click="startEditing" />
        </template>

        <template v-if="editing">
          <div class="fc-form-grid">
            <FcFormField label="Name" span>
              <template #default="{ id: fieldId }">
                <InputText :id="fieldId" v-model="editName" />
              </template>
            </FcFormField>
            <FcFormField label="Description" span>
              <template #default="{ id: fieldId }">
                <Textarea :id="fieldId" v-model="editDescription" rows="3" />
              </template>
            </FcFormField>
            <FcFormField label="Default Base URL" span>
              <template #default="{ id: fieldId }">
                <InputText
                  :id="fieldId"
                  v-model="editDefaultBaseUrl"
                  placeholder="https://example.com"
                />
              </template>
            </FcFormField>
            <FcFormField label="Icon URL">
              <template #default="{ id: fieldId }">
                <InputText
                  :id="fieldId"
                  v-model="editIconUrl"
                  placeholder="https://example.com/icon.png"
                />
              </template>
            </FcFormField>
            <FcFormField label="Website">
              <template #default="{ id: fieldId }">
                <InputText
                  :id="fieldId"
                  v-model="editWebsite"
                  placeholder="https://www.example.com"
                />
              </template>
            </FcFormField>
            <FcFormField label="Logo (SVG)" span>
              <template #default="{ id: fieldId }">
                <Textarea
                  :id="fieldId"
                  v-model="editLogo"
                  rows="4"
                  placeholder="Paste SVG content here"
                />
              </template>
            </FcFormField>
            <FcFormField v-if="editLogo" label="Logo MIME Type">
              <template #default="{ id: fieldId }">
                <InputText
                  :id="fieldId"
                  v-model="editLogoMimeType"
                  placeholder="image/svg+xml"
                />
              </template>
            </FcFormField>
          </div>
        </template>

        <template v-else>
          <div class="fc-detail-grid">
            <FcDetailField label="Code">
              <code>{{ application.code }}</code>
            </FcDetailField>
            <FcDetailField label="Name" :value="application.name" />
            <FcDetailField label="Description" :value="application.description" span />
            <FcDetailField label="Default Base URL" :value="application.defaultBaseUrl" />
            <FcDetailField label="Icon URL" :value="application.iconUrl" />
            <FcDetailField label="Website" :value="application.website" />
            <FcDetailField label="Logo">
              <span v-if="application.logo">{{ application.logoMimeType || 'Configured' }}</span>
              <span v-else>—</span>
            </FcDetailField>
            <FcDetailField label="Created" :value="formatDate(application.createdAt)" />
            <FcDetailField label="Updated" :value="formatDate(application.updatedAt)" />
          </div>
        </template>
      </FcFormSection>

      <!-- Service Account -->
      <FcFormSection title="Service Account" flat>
        <template v-if="application.serviceAccountId">
          <div class="detail-grid">
            <div class="detail-item">
              <label>Status</label>
              <Tag value="Provisioned" severity="success" />
            </div>
            <div class="detail-item">
              <label>Principal ID</label>
              <code>{{ application.serviceAccountId }}</code>
            </div>
          </div>
          <Message severity="info" class="service-account-info">
            Service account credentials are managed in the OAuth Clients section. The client
            secret can only be viewed at creation time or when rotated.
          </Message>
        </template>
        <template v-else>
          <div class="action-item">
            <div class="action-info">
              <strong>Provision Service Account</strong>
              <p>
                Create a service account with OAuth credentials for machine-to-machine
                authentication.
              </p>
            </div>
            <Button
              label="Provision"
              icon="pi pi-plus"
              :loading="provisioning"
              @click="provisionServiceAccount"
            />
          </div>
        </template>
      </FcFormSection>

      <!-- Documentation (synced by the application's SDK) -->
      <FcFormSection title="Documentation" flat>
        <template v-if="appDocs.length > 0">
          <div class="app-docs-list">
            <router-link
              v-for="d in appDocs"
              :key="d.slug"
              :to="`/platform/docs/${application.code}/${d.slug}`"
              class="app-doc-link"
            >
              <i class="pi pi-book"></i>
              {{ d.title }}
            </router-link>
          </div>
        </template>
        <p v-else class="app-docs-empty">
          No documentation synced. The application can publish pages with
          <code>POST /api/applications/{{ application.code }}/docs/sync</code> —
          they render under Platform → Documentation.
        </p>
      </FcFormSection>

      <!-- Login Client -->
      <FcFormSection title="Login Client" flat>
        <template v-if="application.hasLoginClient">
          <div class="detail-grid">
            <div class="detail-item">
              <label>Status</label>
              <Tag value="Provisioned" severity="success" />
            </div>
          </div>
          <Message severity="info" class="service-account-info">
            Login client settings (redirect URIs, allowed origins, secret rotation) are
            managed in the OAuth Clients section.
          </Message>
        </template>
        <template v-else>
          <div class="action-info">
            <strong>Provision Login Client</strong>
            <p>
              Create an OAuth client for user authentication via OIDC (authorization_code
              grant). Required if your application has a UI that users log into.
            </p>
          </div>
          <div class="form-field">
            <label>Client Type</label>
            <Select
              v-model="loginClientType"
              :options="[
                { label: 'PUBLIC — SPA / native app (PKCE only)', value: 'PUBLIC' },
                {
                  label: 'CONFIDENTIAL — server-rendered app (has client secret)',
                  value: 'CONFIDENTIAL',
                },
              ]"
              option-label="label"
              option-value="value"
              class="full-width"
            />
          </div>
          <div class="form-field">
            <label>Redirect URIs *</label>
            <div class="redirect-uri-input">
              <InputText
                v-model="newLoginRedirectUri"
                placeholder="https://app.example.com/callback"
                class="flex-grow"
                @keyup.enter="addLoginRedirectUri"
              />
              <Button
                icon="pi pi-plus"
                @click="addLoginRedirectUri"
                :disabled="!newLoginRedirectUri.trim()"
              />
            </div>
            <div v-if="loginClientRedirectUris.length > 0" class="uri-list">
              <Chip
                v-for="uri in loginClientRedirectUris"
                :key="uri"
                :label="uri"
                removable
                @remove="removeLoginRedirectUri(uri)"
              />
            </div>
            <small class="field-help">
              Allowed callback URLs for OAuth redirects. Add at least one to provision.
            </small>
          </div>
          <Button
            label="Provision Login Client"
            icon="pi pi-plus"
            :disabled="loginClientRedirectUris.length === 0"
            :loading="provisioningLoginClient"
            @click="provisionLoginClient"
          />
        </template>
      </FcFormSection>

      <!-- Actions -->
      <FcFormSection v-if="!editing" title="Actions" flat>
        <div class="action-items">
          <div v-if="!application.active" class="action-item">
            <div class="action-info">
              <strong>Activate Application</strong>
              <p>Make this application available for use.</p>
            </div>
            <Button label="Activate" severity="success" outlined @click="confirmActivate" />
          </div>

          <div v-else class="action-item">
            <div class="action-info">
              <strong>Deactivate Application</strong>
              <p>Prevent new event types from using this application.</p>
            </div>
            <Button label="Deactivate" severity="warn" outlined @click="confirmDeactivate" />
          </div>

          <div class="action-item">
            <div class="action-info">
              <strong>Delete Application</strong>
              <p>Permanently delete this application. Cannot be undone.</p>
            </div>
            <Button
              label="Delete"
              severity="danger"
              outlined
              :disabled="application.active"
              @click="confirmDelete"
            />
          </div>
        </div>
      </FcFormSection>
    </template>

    <template v-if="editing" #footer>
      <FcFormActions :bordered="false">
        <Button v-if="dirty" label="Discard" severity="secondary" outlined @click="cancelEditing" />
        <Button label="Save" :disabled="!dirty" :loading="saving" @click="saveChanges" />
      </FcFormActions>
    </template>
  </EntityDrawer>

  <!-- Secret-shown-once dialogs live OUTSIDE the drawer body: they teleport to
       body (stacking above the drawer) and must stay mounted through the
       post-provision reload, when EntityDrawer swaps its slot for a spinner. -->

  <!-- Service Account Credentials Dialog -->
  <Dialog
    v-model:visible="showCredentialsDialog"
    header="Service Account Provisioned"
    :style="{ width: '550px' }"
    :modal="true"
    :closable="false"
  >
    <div class="credentials-dialog-content" v-if="provisionedCredentials">
      <Message severity="warn" class="credentials-warning">
        Save these credentials now. The client secret will not be shown again.
      </Message>

      <div class="credential-item">
        <label>Client ID</label>
        <div class="credential-value">
          <code>{{ provisionedCredentials.oauthClient.clientId }}</code>
          <Button
            icon="pi pi-copy"
            text
            size="small"
            @click="copyToClipboard(provisionedCredentials.oauthClient.clientId)"
          />
        </div>
      </div>

      <div class="credential-item">
        <label>Client Secret</label>
        <div class="credential-value">
          <code>{{ provisionedCredentials.oauthClient.clientSecret }}</code>
          <!-- clientSecret is optional on the shared credentials wire shape
               (PUBLIC login clients have none); service-account provisioning
               always returns one. -->
          <Button
            icon="pi pi-copy"
            text
            size="small"
            @click="copyToClipboard(provisionedCredentials.oauthClient.clientSecret ?? '')"
          />
        </div>
      </div>

      <div class="credential-item">
        <label>Service Account</label>
        <div class="credential-value">
          <span>{{ provisionedCredentials.name }}</span>
        </div>
      </div>
    </div>

    <template #footer>
      <Button
        label="I've saved the credentials"
        icon="pi pi-check"
        @click="onCredentialsDialogClose"
      />
    </template>
  </Dialog>

  <!-- Login Client Credentials Dialog -->
  <Dialog
    v-model:visible="showLoginClientDialog"
    header="Login Client Provisioned"
    :style="{ width: '550px' }"
    :modal="true"
    :closable="false"
  >
    <div class="credentials-dialog-content" v-if="provisionedLoginClient">
      <Message
        v-if="provisionedLoginClient.clientType === 'CONFIDENTIAL'"
        severity="warn"
        class="credentials-warning"
      >
        Save these credentials now. The client secret will not be shown again.
      </Message>
      <Message v-else severity="info" class="credentials-warning">
        PUBLIC clients use PKCE — there is no client secret. Configure your app with the
        client ID below.
      </Message>

      <div class="credential-item">
        <label>Client ID</label>
        <div class="credential-value">
          <code>{{ provisionedLoginClient.oauthClient.clientId }}</code>
          <Button
            icon="pi pi-copy"
            text
            size="small"
            @click="copyToClipboard(provisionedLoginClient.oauthClient.clientId)"
          />
        </div>
      </div>

      <div
        v-if="provisionedLoginClient.oauthClient.clientSecret"
        class="credential-item"
      >
        <label>Client Secret</label>
        <div class="credential-value">
          <code>{{ provisionedLoginClient.oauthClient.clientSecret }}</code>
          <Button
            icon="pi pi-copy"
            text
            size="small"
            @click="
              copyToClipboard(provisionedLoginClient.oauthClient.clientSecret ?? '')
            "
          />
        </div>
      </div>

      <div class="credential-item">
        <label>Client Type</label>
        <div class="credential-value">
          <span>{{ provisionedLoginClient.clientType }}</span>
        </div>
      </div>

      <div class="credential-item">
        <label>Redirect URIs</label>
        <div class="credential-value">
          <code>{{ provisionedLoginClient.redirectUris.join(', ') }}</code>
        </div>
      </div>
    </div>

    <template #footer>
      <Button
        label="I've saved the credentials"
        icon="pi pi-check"
        @click="onLoginClientDialogClose"
      />
    </template>
  </Dialog>
</template>

<style scoped>
/* Service Account / Login Client provisioned status (ported verbatim) */
.detail-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 20px;
}

.detail-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.detail-item label {
  font-size: 12px;
  font-weight: 500;
  color: #64748b;
  text-transform: uppercase;
}

.form-field {
  margin-bottom: 20px;
}

.form-field label {
  display: block;
  margin-bottom: 6px;
  font-weight: 500;
}

.full-width {
  width: 100%;
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

.field-help {
  color: #64748b;
  font-size: 12px;
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

.service-account-info {
  margin-top: 16px;
}

/* Credentials Dialog */
.credentials-dialog-content {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.credentials-warning {
  margin-bottom: 8px;
}

.credential-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.credential-item > label {
  font-size: 12px;
  font-weight: 500;
  color: #64748b;
  text-transform: uppercase;
}

.credential-value {
  display: flex;
  align-items: center;
  gap: 8px;
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 6px;
  padding: 8px 12px;
}

.credential-value code {
  font-family: 'JetBrains Mono', monospace;
  font-size: 13px;
  flex: 1;
  word-break: break-all;
}

@media (max-width: 640px) {
  .detail-grid {
    grid-template-columns: 1fr;
  }
}

.app-docs-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.app-doc-link {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 10px;
  border-radius: 6px;
  color: var(--primary-color, #4f46e5);
  text-decoration: none;
  font-size: 13.5px;
}

.app-doc-link:hover {
  background: var(--surface-hover, #f1f5f9);
}

.app-doc-link .pi {
  font-size: 13px;
  color: var(--text-color-secondary, #64748b);
}

.app-docs-empty {
  color: var(--text-color-secondary, #64748b);
  font-size: 13.5px;
  margin: 0;
}

.app-docs-empty code {
  background: var(--surface-ground, #f1f5f9);
  padding: 1px 5px;
  border-radius: 4px;
  font-size: 0.9em;
}
</style>
