<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, onMounted } from "vue";
import { useRouter } from "vue-router";
import {
	serviceAccountsApi,
	type CreateServiceAccountResponse,
} from "@/api/service-accounts";
import type { PrincipalScope } from "@/api/users";
import { clientsApi, type Client } from "@/api/clients";

const router = useRouter();

const code = ref("");
const name = ref("");
const description = ref("");
const scope = ref<PrincipalScope>("ANCHOR");
const selectedClientIds = ref<string[]>([]);
const clients = ref<Client[]>([]);
const saving = ref(false);

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
	if (!code.value.trim() || !name.value.trim()) return false;
	// The scope must agree with the client links (the API rejects otherwise).
	if (scope.value === "CLIENT") return selectedClientIds.value.length === 1;
	if (scope.value === "PARTNER") return selectedClientIds.value.length > 0;
	return true;
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
		toast.error(
			"Error",
			"Code and name are required; a Client account needs exactly one client, a Partner account at least one",
		);
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
				// An ANCHOR account reaches every client and takes none; a
				// selection left over from another scope isn't sent.
				clientIds:
					scope.value !== "ANCHOR" && selectedClientIds.value.length > 0
						? selectedClientIds.value
						: undefined,
			});

		// Store credentials and show dialog
		createdCredentials.value = {
			clientId: response.oauth.clientId,
			clientSecret: response.oauth.clientSecret,
			authToken: response.webhook.authToken,
			signingSecret: response.webhook.signingSecret,
		};
		createdServiceAccountId.value = response.serviceAccount.id;
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
		router.push(`/identity/service-accounts/${createdServiceAccountId.value}`);
	} else {
		router.push("/identity/service-accounts");
	}
}

function goBack() {
	router.push("/identity/service-accounts");
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div class="header-left">
        <Button
          icon="pi pi-arrow-left"
          text
          rounded
          severity="secondary"
          @click="goBack"
          v-tooltip.right="'Back to service accounts'"
        />
        <div>
          <h1 class="page-title">Create Service Account</h1>
          <p class="page-subtitle">Create a new service account with webhook credentials</p>
        </div>
      </div>
    </header>

    <div class="fc-card form-card">
      <div class="form-section">
        <h2 class="section-title">Basic Information</h2>

        <div class="form-group">
          <label for="name">Name <span class="required">*</span></label>
          <InputText
            id="name"
            v-model="name"
            placeholder="My Service Account"
            class="w-full"
            @blur="generateCode"
          />
          <small class="help-text">A human-readable name for this service account</small>
        </div>

        <div class="form-group">
          <label for="code">Code <span class="required">*</span></label>
          <InputText id="code" v-model="code" placeholder="my-service-account" class="w-full" />
          <small class="help-text">
            Unique identifier (lowercase, alphanumeric with dashes). Example: tms-service
          </small>
        </div>

        <div class="form-group">
          <label for="description">Description</label>
          <Textarea
            id="description"
            v-model="description"
            placeholder="Optional description..."
            rows="3"
            class="w-full"
          />
        </div>

        <div class="form-group">
          <label for="scope">Scope <span class="required">*</span></label>
          <Select
            id="scope"
            v-model="scope"
            :options="scopeOptions"
            optionLabel="label"
            optionValue="value"
            class="w-full"
          />
          <small class="help-text">
            Determines which clients this service account can access.
          </small>
        </div>

        <div class="form-group" v-if="scope !== 'ANCHOR'">
          <label for="clients">Client Access</label>
          <MultiSelect
            id="clients"
            v-model="selectedClientIds"
            :options="clientOptions"
            optionLabel="label"
            optionValue="value"
            placeholder="Select clients..."
            display="chip"
            filter
            class="w-full"
          />
          <small class="help-text">
            {{
              scope === "CLIENT"
                ? "Select the one client this service account can access."
                : "Select which clients this service account can access."
            }}
          </small>
        </div>
      </div>

      <p class="help-text">
        A new service account has no application access. Grant applications on its detail
        page once it is created.
      </p>

      <div class="form-actions">
        <Button label="Cancel" text severity="secondary" @click="goBack" />
        <Button
          label="Create Service Account"
          icon="pi pi-check"
          :disabled="!isValid"
          :loading="saving"
          @click="createServiceAccount"
        />
      </div>
    </div>

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
  </div>
</template>

<style scoped>
.header-left {
  display: flex;
  align-items: center;
  gap: 12px;
}

.form-card {
  max-width: 600px;
}

.form-section {
  margin-bottom: 24px;
}

.section-title {
  font-size: 16px;
  font-weight: 600;
  color: #1e293b;
  margin: 0 0 20px 0;
}

.form-group {
  margin-bottom: 20px;
}

.form-group label {
  display: block;
  font-size: 14px;
  font-weight: 500;
  color: #1e293b;
  margin-bottom: 6px;
}

.form-group .required {
  color: #ef4444;
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

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: 12px;
  padding-top: 20px;
  border-top: 1px solid #e2e8f0;
}

.w-full {
  width: 100%;
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

@media (max-width: 768px) {
  .form-card {
    max-width: 100%;
  }
}
</style>
