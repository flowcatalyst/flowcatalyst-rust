<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, onMounted } from "vue";
import {
	serviceAccountsApi,
	type CreateServiceAccountResponse,
} from "@/api/service-accounts";
import type { PrincipalScope } from "@/api/users";
import { clientsApi, type Client } from "@/api/clients";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";

const emit = defineEmits<{
	changed: [];
}>();

const code = ref("");
const name = ref("");
const description = ref("");
const scope = ref<PrincipalScope>("ANCHOR");
const selectedClientIds = ref<string[]>([]);
// Off by default: a new account has no application access until granted.
const allApplications = ref(false);
const clients = ref<Client[]>([]);
const saving = ref(false);

// Once the account exists the drawer must never block navigation — the
// secret-once credentials dialog is the only remaining step.
const created = ref(false);
const dirty = computed(
	() =>
		!created.value &&
		(!!name.value || !!code.value || !!description.value),
);

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { goToList, replaceToDetail } = useDrawerRoute({
	listPath: "/identity/service-accounts",
	dirty,
});

const scopeOptions = [
	{ label: "Anchor (all clients)", value: "ANCHOR" },
	{ label: "Partner (assigned clients)", value: "PARTNER" },
	{ label: "Client (single client)", value: "CLIENT" },
];

// Created credentials dialog
const showCredentialsDialog = ref(false);
const createdCredentials = ref<{
	clientId: string;
	clientSecret: string;
	authToken: string;
	signingSecret: string;
} | null>(null);
const createdServiceAccountId = ref<string | null>(null);

const isValid = computed(() => {
	return code.value.trim() && name.value.trim();
});

const clientOptions = computed(() => {
	return clients.value.map((c) => ({
		label: c.name,
		value: c.id,
	}));
});

onMounted(async () => {
	await loadClients();
});

async function loadClients() {
	try {
		const response = await clientsApi.list();
		clients.value = response.clients;
	} catch (error) {
		console.error("Failed to fetch clients:", error);
	}
}

function generateCode() {
	// Generate a code from the name (lowercase, replace spaces with dashes, remove special chars)
	if (name.value) {
		code.value = name.value
			.toLowerCase()
			.replace(/\s+/g, "-")
			.replace(/[^a-z0-9-]/g, "")
			.replace(/-+/g, "-")
			.replace(/^-|-$/g, "");
	}
}

async function createServiceAccount() {
	if (!isValid.value) {
		toast.error("Error", "Code and name are required");
		return;
	}

	saving.value = true;
	try {
		const response: CreateServiceAccountResponse =
			await serviceAccountsApi.create({
				code: code.value,
				name: name.value,
				description: description.value || undefined,
				scope: scope.value,
				clientIds:
					selectedClientIds.value.length > 0
						? selectedClientIds.value
						: undefined,
				allApplications: allApplications.value || undefined,
			});

		// Store credentials and show the secret-once dialog; navigation waits
		// until the user confirms they copied the credentials.
		createdCredentials.value = {
			clientId: response.oauth.clientId,
			clientSecret: response.oauth.clientSecret,
			authToken: response.webhook.authToken,
			signingSecret: response.webhook.signingSecret,
		};
		createdServiceAccountId.value = response.serviceAccount.id;
		created.value = true;
		emit("changed");
		showCredentialsDialog.value = true;

		toast.success("Success", "Service account created successfully");
	} catch (e: unknown) {
	} finally {
		saving.value = false;
	}
}

function copyToClipboard(text: string, label: string) {
	navigator.clipboard.writeText(text);
	toast.info("Copied", `${label} copied to clipboard`);
}

function closeDialogAndNavigate() {
	showCredentialsDialog.value = false;
	if (createdServiceAccountId.value) {
		replaceToDetail(createdServiceAccountId.value);
	} else {
		goToList();
	}
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    title="Create Service Account"
    subtitle="Create a new service account with webhook credentials"
    :dirty="dirty"
    @close="goToList()"
  >
    <FcFormSection title="Basic Information" flat>
      <div class="fc-form-grid">
        <FcFormField
          label="Name"
          required
          span
          help="A human-readable name for this service account"
        >
          <template #default="{ id: fieldId }">
            <InputText
              :id="fieldId"
              v-model="name"
              placeholder="My Service Account"
              @blur="generateCode"
            />
          </template>
        </FcFormField>

        <FcFormField
          label="Code"
          required
          span
          help="Unique identifier (lowercase, alphanumeric with dashes). Example: tms-service"
        >
          <template #default="{ id: fieldId }">
            <InputText :id="fieldId" v-model="code" placeholder="my-service-account" />
          </template>
        </FcFormField>

        <FcFormField label="Description" span>
          <template #default="{ id: fieldId }">
            <Textarea
              :id="fieldId"
              v-model="description"
              placeholder="Optional description..."
              rows="3"
            />
          </template>
        </FcFormField>

        <FcFormField
          label="Scope"
          required
          span
          help="Determines which clients this service account can access."
        >
          <template #default="{ id: fieldId }">
            <Select
              :id="fieldId"
              v-model="scope"
              :options="scopeOptions"
              optionLabel="label"
              optionValue="value"
            />
          </template>
        </FcFormField>

        <FcFormField
          v-if="scope !== 'ANCHOR'"
          label="Client Access"
          span
          help="Select which clients this service account can access."
        >
          <template #default="{ id: fieldId }">
            <MultiSelect
              :id="fieldId"
              v-model="selectedClientIds"
              :options="clientOptions"
              optionLabel="label"
              optionValue="value"
              placeholder="Select clients..."
              display="chip"
              filter
            />
          </template>
        </FcFormField>
      </div>
    </FcFormSection>

    <FcFormSection title="Application Access" flat>
      <div class="all-apps-toggle">
        <ToggleSwitch inputId="createAllApplications" v-model="allApplications" />
        <label for="createAllApplications" class="all-apps-label">
          <span class="all-apps-title">Access to all applications</span>
          <span class="all-apps-hint">
            Leave off to start with no application access; grant specific
            applications from the account's detail view after creating it.
          </span>
        </label>
      </div>
    </FcFormSection>

    <!-- Credentials Dialog (shown once after creation) -->
    <Dialog
      v-model:visible="showCredentialsDialog"
      header="Service Account Created"
      :style="{ width: '650px' }"
      :modal="true"
      :closable="false"
    >
      <div class="credentials-dialog">
        <div class="warning-banner">
          <i class="pi pi-exclamation-triangle"></i>
          <span>Copy these credentials now. They will not be shown again.</span>
        </div>

        <div class="credentials-group">
          <h3 class="credentials-group-title">OAuth Credentials (API Authentication)</h3>
          <p class="credentials-group-desc">
            Use these for client_credentials grant to obtain access tokens.
          </p>

          <div class="credential-section">
            <label>Client ID</label>
            <div class="credential-value">
              <code>{{ createdCredentials?.clientId }}</code>
              <Button
                icon="pi pi-copy"
                text
                rounded
                @click="copyToClipboard(createdCredentials?.clientId!, 'Client ID')"
                v-tooltip.top="'Copy'"
              />
            </div>
          </div>

          <div class="credential-section">
            <label>Client Secret</label>
            <div class="credential-value">
              <code>{{ createdCredentials?.clientSecret }}</code>
              <Button
                icon="pi pi-copy"
                text
                rounded
                @click="copyToClipboard(createdCredentials?.clientSecret!, 'Client Secret')"
                v-tooltip.top="'Copy'"
              />
            </div>
          </div>
        </div>

        <div class="credentials-group">
          <h3 class="credentials-group-title">Webhook Credentials</h3>
          <p class="credentials-group-desc">
            Use these for outbound webhook authentication and signature verification.
          </p>

          <div class="credential-section">
            <label>Auth Token (Bearer)</label>
            <div class="credential-value">
              <code>{{ createdCredentials?.authToken }}</code>
              <Button
                icon="pi pi-copy"
                text
                rounded
                @click="copyToClipboard(createdCredentials?.authToken!, 'Auth Token')"
                v-tooltip.top="'Copy'"
              />
            </div>
            <small class="help-text">
              Sent in the Authorization header: <code>Authorization: Bearer &lt;token&gt;</code>
            </small>
          </div>

          <div class="credential-section">
            <label>Signing Secret</label>
            <div class="credential-value">
              <code>{{ createdCredentials?.signingSecret }}</code>
              <Button
                icon="pi pi-copy"
                text
                rounded
                @click="copyToClipboard(createdCredentials?.signingSecret!, 'Signing Secret')"
                v-tooltip.top="'Copy'"
              />
            </div>
            <small class="help-text"> Used to verify webhook signatures (HMAC-SHA256) </small>
          </div>
        </div>
      </div>

      <template #footer>
        <Button
          label="I've Copied the Credentials"
          icon="pi pi-check"
          @click="closeDialogAndNavigate"
        />
      </template>
    </Dialog>

    <template #footer>
      <FcFormActions :bordered="false">
        <Button
          label="Cancel"
          severity="secondary"
          outlined
          :disabled="saving"
          @click="drawer?.close()"
        />
        <Button
          label="Create Service Account"
          icon="pi pi-check"
          :disabled="!isValid"
          :loading="saving"
          @click="createServiceAccount"
        />
      </FcFormActions>
    </template>
  </EntityDrawer>
</template>

<style scoped>
.all-apps-toggle {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 4px 0 16px 0;
}

.all-apps-label {
  display: flex;
  flex-direction: column;
  gap: 2px;
  cursor: pointer;
}

.all-apps-title {
  font-weight: 600;
  font-size: 14px;
}

.all-apps-hint {
  font-size: 12px;
  color: #64748b;
}

.help-text {
  display: block;
  font-size: 12px;
  color: #64748b;
  margin-top: 4px;
}

.help-text code {
  background: #f1f5f9;
  padding: 1px 4px;
  border-radius: 3px;
  font-size: 11px;
}

/* Credentials Dialog */
.credentials-dialog {
  display: flex;
  flex-direction: column;
  gap: 20px;
}

.warning-banner {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  background: #fffbeb;
  border: 1px solid #fcd34d;
  border-radius: 8px;
  color: #92400e;
}

.warning-banner i {
  font-size: 20px;
  color: #f59e0b;
}

.credential-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.credential-section label {
  font-size: 14px;
  font-weight: 600;
  color: #475569;
}

.credential-value {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px;
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 6px;
}

.credential-value code {
  flex: 1;
  font-size: 12px;
  word-break: break-all;
  color: #1e293b;
}

.credential-section .help-text {
  margin-top: 0;
}

.credentials-group {
  padding: 16px;
  background: #f8fafc;
  border-radius: 8px;
  border: 1px solid #e2e8f0;
}

.credentials-group-title {
  font-size: 14px;
  font-weight: 600;
  color: #1e293b;
  margin: 0 0 4px 0;
}

.credentials-group-desc {
  font-size: 12px;
  color: #64748b;
  margin: 0 0 16px 0;
}

.credentials-group .credential-section {
  background: white;
  padding: 12px;
  border-radius: 6px;
  margin-bottom: 12px;
}

.credentials-group .credential-section:last-child {
  margin-bottom: 0;
}

.credentials-group .credential-value {
  background: #f1f5f9;
}
</style>
