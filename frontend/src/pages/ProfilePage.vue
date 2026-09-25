<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useAuthStore } from "@/stores/auth";
import { toast } from "@/utils/errorBus";
import { getErrorMessage } from "@/utils/errors";
import { passwordPolicyError } from "@/utils/passwordPolicy";
import { useConfirm } from "primevue/useconfirm";
import {
	changePassword,
	sendChangePasswordEmailCode,
} from "@/api/changePassword";
import { usersApi } from "@/api/users";
import PasskeysSection from "@/components/PasskeysSection.vue";
import TwoFactorSection from "@/components/TwoFactorSection.vue";

const authStore = useAuthStore();
const confirm = useConfirm();

// ── Change password ─────────────────────────────────────────────────────────
const showChangePassword = ref(false);
const cpCurrent = ref("");
const cpNew = ref("");
const cpConfirm = ref("");
const cpCode = ref("");
const cpMfaRequired = ref(false);
const cpMethods = ref<string[]>([]);
const cpBusy = ref(false);
const cpError = ref("");

const cpHasEmailFactor = computed(() => cpMethods.value.includes("EMAIL_PIN"));
const cpHasTotpFactor = computed(() => cpMethods.value.includes("TOTP"));

function openChangePassword() {
	cpCurrent.value = "";
	cpNew.value = "";
	cpConfirm.value = "";
	cpCode.value = "";
	cpMfaRequired.value = false;
	cpMethods.value = [];
	cpError.value = "";
	showChangePassword.value = true;
}

async function submitChangePassword() {
	cpError.value = "";
	const policyErr = passwordPolicyError(cpNew.value, {
		email: authStore.user?.email,
		name: authStore.user?.name,
	});
	if (policyErr) {
		cpError.value = policyErr;
		return;
	}
	if (cpNew.value !== cpConfirm.value) {
		cpError.value = "New password and confirmation do not match.";
		return;
	}
	if (cpMfaRequired.value && !cpCode.value.trim()) {
		cpError.value = "Enter your two-factor code.";
		return;
	}
	cpBusy.value = true;
	try {
		const result = await changePassword({
			currentPassword: cpCurrent.value,
			newPassword: cpNew.value,
			code: cpMfaRequired.value ? cpCode.value.trim() : undefined,
		});
		if (result.ok) {
			toast.success("Password changed", "Your password has been updated.");
			showChangePassword.value = false;
			return;
		}
		if (result.mfaRequired) {
			// Reveal the 2FA step. For an email factor, send the code right away.
			cpMfaRequired.value = true;
			cpMethods.value = result.methods ?? [];
			if (cpMethods.value.includes("EMAIL_PIN") && !cpMethods.value.includes("TOTP")) {
				await sendEmailCode();
			}
			cpError.value = result.message ?? "Two-factor verification required.";
			return;
		}
		cpError.value = result.message ?? "Could not change your password.";
	} catch (e) {
		cpError.value = getErrorMessage(e, "Could not change your password.");
	} finally {
		cpBusy.value = false;
	}
}

async function sendEmailCode() {
	try {
		const r = await sendChangePasswordEmailCode();
		toast.success("Code sent", r.message);
	} catch (e) {
		toast.error("Send failed", getErrorMessage(e, "Could not send a code."));
	}
}

// ── Session / sign-in activity ──────────────────────────────────────────────
interface LoginHistoryItem {
	attemptType: string;
	outcome: string;
	failureReason?: string;
	ipAddress?: string;
	userAgent?: string;
	attemptedAt: string;
}

const showSessions = ref(false);
const sessionsLoading = ref(false);
const attempts = ref<LoginHistoryItem[]>([]);

async function openSessions() {
	showSessions.value = true;
	sessionsLoading.value = true;
	attempts.value = [];
	try {
		const res = await fetch("/auth/login-history", { credentials: "include" });
		if (!res.ok) throw new Error("Could not load sign-in activity.");
		const data = await res.json();
		attempts.value = data.attempts ?? [];
	} catch (e) {
		toast.error("Error", getErrorMessage(e, "Could not load sign-in activity."));
	} finally {
		sessionsLoading.value = false;
	}
}

function formatWhen(iso: string): string {
	return new Date(iso).toLocaleString();
}

// Condense a user-agent into a short "Browser on OS" label.
function deviceLabel(ua?: string): string {
	if (!ua) return "Unknown device";
	const browser = /Edg\//.test(ua)
		? "Edge"
		: /Chrome\//.test(ua)
			? "Chrome"
			: /Firefox\//.test(ua)
				? "Firefox"
				: /Safari\//.test(ua)
					? "Safari"
					: "Browser";
	const os = /Windows/.test(ua)
		? "Windows"
		: /Mac OS X|Macintosh/.test(ua)
			? "macOS"
			: /Android/.test(ua)
				? "Android"
				: /iPhone|iPad|iOS/.test(ua)
					? "iOS"
					: /Linux/.test(ua)
						? "Linux"
						: "";
	return os ? `${browser} on ${os}` : browser;
}

function outcomeOk(outcome: string): boolean {
	return outcome.toUpperCase() === "SUCCESS";
}

// ── Developer API access (self-service client_credentials) ───────────────
// Matches internal/platform/seed/permissions.go's permDeveloperAPICredentialManage
// — granted only via the seeded "platform:developer" role.
const DEV_CREDENTIAL_PERMISSION = "platform:developer:api-credential:manage";

const canManageDevCredential = computed(() =>
	(authStore.user?.permissions || []).includes(DEV_CREDENTIAL_PERMISSION),
);

const oauthTokenUrl = computed(() => `${window.location.origin}/oauth/token`);

const devCredentialLoading = ref(true);
const hasDevCredential = ref(false);
const devCredentialUpdatedAt = ref<string | null>(null);
const devCredentialBusy = ref(false);
const showDevSecretDialog = ref(false);
const newDevSecret = ref<string | null>(null);

onMounted(async () => {
	if (!canManageDevCredential.value || !authStore.user) {
		devCredentialLoading.value = false;
		return;
	}
	try {
		const p = await usersApi.get(authStore.user.id);
		hasDevCredential.value = p.hasDeveloperCredential;
		devCredentialUpdatedAt.value = p.developerCredentialUpdatedAt ?? null;
	} catch (e) {
		toast.error("Error", getErrorMessage(e, "Could not load your developer credential status."));
	} finally {
		devCredentialLoading.value = false;
	}
});

async function rotateDevCredential() {
	if (!authStore.user) return;
	devCredentialBusy.value = true;
	try {
		const response = await usersApi.setDeveloperCredential(authStore.user.id);
		newDevSecret.value = response.clientSecret ?? null;
		hasDevCredential.value = true;
		devCredentialUpdatedAt.value = new Date().toISOString();
		showDevSecretDialog.value = true;
		toast.success("Success", "Developer client secret updated");
	} catch (e) {
		toast.error("Error", getErrorMessage(e, "Could not set your developer credential."));
	} finally {
		devCredentialBusy.value = false;
	}
}

function confirmRevokeDevCredential() {
	confirm.require({
		message: "Revoke your developer client secret? Any scripts using it will stop working immediately.",
		header: "Revoke Credential",
		icon: "pi pi-exclamation-triangle",
		acceptClass: "p-button-danger",
		accept: () => revokeDevCredential(),
	});
}

async function revokeDevCredential() {
	if (!authStore.user) return;
	devCredentialBusy.value = true;
	try {
		await usersApi.revokeDeveloperCredential(authStore.user.id);
		hasDevCredential.value = false;
		devCredentialUpdatedAt.value = null;
		toast.success("Revoked", "Developer client secret revoked");
	} catch (e) {
		toast.error("Error", getErrorMessage(e, "Could not revoke your developer credential."));
	} finally {
		devCredentialBusy.value = false;
	}
}

function copyDevSecret(text: string) {
	navigator.clipboard.writeText(text);
	toast.info("Copied", "Client secret copied to clipboard");
}

function formatDevCredentialDate(iso: string | null): string {
	if (!iso) return "Never";
	return new Date(iso).toLocaleString();
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Profile</h1>
        <p class="page-subtitle">Manage your account settings</p>
      </div>
    </header>

    <div class="profile-grid">
      <!-- User Info Card -->
      <FcFormSection title="User Information">
        <div class="profile-info">
          <div class="avatar-large">
            {{ authStore.userInitials }}
          </div>
          <div class="user-details">
            <h3>{{ authStore.displayName }}</h3>
            <p>{{ authStore.user?.email }}</p>
            <div class="roles-list">
              <Tag
                v-for="role in authStore.user?.roles || []"
                :key="role"
                :value="role"
                class="role-tag"
              />
            </div>
          </div>
        </div>
      </FcFormSection>

      <!-- Account Settings Card -->
      <FcFormSection title="Account Settings">
        <div class="fc-form-grid">
          <FcFormField label="Display Name" span>
            <template #default="{ id: fieldId }">
              <InputText :id="fieldId" :value="authStore.displayName" disabled />
            </template>
          </FcFormField>
          <FcFormField label="Email" span>
            <template #default="{ id: fieldId }">
              <InputText :id="fieldId" :value="authStore.user?.email" disabled />
            </template>
          </FcFormField>
        </div>
        <p v-if="authStore.user?.ssoManaged" class="sso-managed-note">
          <i class="pi pi-lock"></i>
          Your password is managed by your organisation's identity provider.
        </p>
        <FcFormActions v-else>
          <Button
            label="Change Password"
            icon="pi pi-key"
            outlined
            @click="openChangePassword"
          />
        </FcFormActions>
      </FcFormSection>

      <!-- Passkeys Card -->
      <PasskeysSection />

      <!-- Two-Factor Authentication Card -->
      <TwoFactorSection />

      <!-- Developer API Access Card -->
      <FcFormSection v-if="canManageDevCredential" title="Developer API Access">
        <div v-if="devCredentialLoading" class="dev-cred-loading">
          <ProgressSpinner strokeWidth="3" style="width: 24px; height: 24px" />
        </div>
        <div v-else class="dev-cred-body">
          <p class="dev-cred-intro">
            Mint a client_credentials token as yourself to call this environment from local
            scripts or services — no OAuth client, no Docker stack, just your own secret.
          </p>
          <div class="dev-cred-status">
            <Tag
              :value="hasDevCredential ? 'Credential set' : 'No credential set'"
              :severity="hasDevCredential ? 'success' : 'secondary'"
            />
            <span class="dev-cred-updated">Last rotated: {{ formatDevCredentialDate(devCredentialUpdatedAt) }}</span>
          </div>
          <div class="dev-cred-curl">
            <code>curl -X POST {{ oauthTokenUrl }} \</code><br />
            <code>&nbsp;&nbsp;-d grant_type=client_credentials \</code><br />
            <code>&nbsp;&nbsp;-d client_id={{ authStore.user?.id }} \</code><br />
            <code>&nbsp;&nbsp;-d client_secret=&lt;your secret&gt;</code>
          </div>
          <FcFormActions>
            <Button
              v-if="hasDevCredential"
              label="Revoke"
              icon="pi pi-ban"
              outlined
              severity="danger"
              :loading="devCredentialBusy"
              @click="confirmRevokeDevCredential"
            />
            <Button
              :label="hasDevCredential ? 'Rotate Secret' : 'Set Secret'"
              icon="pi pi-key"
              outlined
              :loading="devCredentialBusy"
              @click="rotateDevCredential"
            />
          </FcFormActions>
        </div>
      </FcFormSection>

      <!-- Security Card -->
      <FcFormSection title="Security">
        <div class="security-info">
          <div class="security-item">
            <div class="security-icon">
              <i class="pi pi-clock"></i>
            </div>
            <div class="security-details">
              <h4>Sign-in Activity</h4>
              <p>Review recent sign-ins to your account</p>
            </div>
            <Button label="View" outlined size="small" @click="openSessions" />
          </div>
        </div>
      </FcFormSection>
    </div>

    <!-- Change Password dialog -->
    <Dialog
      v-model:visible="showChangePassword"
      header="Change Password"
      modal
      :style="{ width: '28rem' }"
    >
      <div class="cp-form">
        <FcFormField label="Current password">
          <template #default="{ id: fieldId }">
            <Password
              :inputId="fieldId"
              v-model="cpCurrent"
              toggleMask
              :feedback="false"
              :inputProps="{ autocomplete: 'current-password' }"
              inputClass="w-full"
              class="w-full"
            />
          </template>
        </FcFormField>
        <FcFormField label="New password">
          <template #default="{ id: fieldId }">
            <Password
              :inputId="fieldId"
              v-model="cpNew"
              toggleMask
              :inputProps="{ autocomplete: 'new-password' }"
              inputClass="w-full"
              class="w-full"
            />
          </template>
        </FcFormField>
        <FcFormField label="Confirm new password">
          <template #default="{ id: fieldId }">
            <Password
              :inputId="fieldId"
              v-model="cpConfirm"
              toggleMask
              :feedback="false"
              :inputProps="{ autocomplete: 'new-password' }"
              inputClass="w-full"
              class="w-full"
            />
          </template>
        </FcFormField>

        <FcFormField v-if="cpMfaRequired" label="Two-factor code" class="cp-mfa">
          <template #default="{ id: fieldId }">
            <div class="cp-mfa-body">
              <InputText
                :id="fieldId"
                v-model="cpCode"
                placeholder="Enter your code"
                autocomplete="one-time-code"
              />
              <small v-if="cpHasTotpFactor" class="cp-hint">
                Enter the code from your authenticator app{{ cpHasEmailFactor ? ", or send an email code below." : "." }}
              </small>
              <small v-else-if="cpHasEmailFactor" class="cp-hint">We sent a code to your email.</small>
              <Button
                v-if="cpHasEmailFactor"
                label="Send email code"
                icon="pi pi-envelope"
                text
                size="small"
                class="cp-send"
                @click="sendEmailCode"
              />
            </div>
          </template>
        </FcFormField>

        <p v-if="cpError" class="cp-error">{{ cpError }}</p>
      </div>
      <template #footer>
        <Button label="Cancel" text severity="secondary" @click="showChangePassword = false" />
        <Button
          :label="cpMfaRequired ? 'Verify & Change' : 'Change Password'"
          icon="pi pi-check"
          :loading="cpBusy"
          @click="submitChangePassword"
        />
      </template>
    </Dialog>

    <!-- Developer client secret reveal dialog -->
    <Dialog
      v-model:visible="showDevSecretDialog"
      header="Developer Client Secret"
      modal
      :style="{ width: '34rem' }"
    >
      <div class="dev-secret-dialog">
        <p class="dev-secret-warning">
          <i class="pi pi-exclamation-triangle"></i>
          Copy this secret now. It will not be shown again.
        </p>
        <div class="dev-secret-value">
          <code>{{ newDevSecret }}</code>
          <Button
            icon="pi pi-copy"
            text
            rounded
            v-tooltip.top="'Copy'"
            @click="copyDevSecret(newDevSecret!)"
          />
        </div>
      </div>
      <template #footer>
        <Button label="Done" @click="showDevSecretDialog = false" />
      </template>
    </Dialog>

    <!-- Sign-in activity dialog -->
    <Dialog
      v-model:visible="showSessions"
      header="Recent sign-in activity"
      modal
      :style="{ width: '40rem' }"
    >
      <div v-if="sessionsLoading" class="sessions-loading">
        <ProgressSpinner strokeWidth="3" />
      </div>
      <div v-else>
        <p class="sessions-note">
          Sessions last up to 24 hours and can't be signed out individually. If you
          see activity you don't recognise, change your password.
        </p>
        <DataTable :value="attempts" size="small" stripedRows class="sessions-table">
          <Column header="When">
            <template #body="{ data }">
              <span>{{ formatWhen(data.attemptedAt) }}</span>
            </template>
          </Column>
          <Column header="Result">
            <template #body="{ data }">
              <Tag
                :value="outcomeOk(data.outcome) ? 'Success' : 'Failed'"
                :severity="outcomeOk(data.outcome) ? 'success' : 'danger'"
              />
            </template>
          </Column>
          <Column header="Device">
            <template #body="{ data }">
              <span v-tooltip.top="data.userAgent">{{ deviceLabel(data.userAgent) }}</span>
            </template>
          </Column>
          <Column header="IP">
            <template #body="{ data }">
              <span class="sessions-ip">{{ data.ipAddress || '—' }}</span>
            </template>
          </Column>
          <template #empty>
            <div class="sessions-empty">No recent sign-in activity.</div>
          </template>
        </DataTable>
      </div>
      <template #footer>
        <Button label="Close" text @click="showSessions = false" />
      </template>
    </Dialog>
  </div>
</template>

<style scoped>
.profile-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(400px, 1fr));
  gap: 24px;
}

/* The grid's gap provides spacing; the section's own bottom margin would
   otherwise make its card shorter than plain-card neighbours in the row. */
.profile-grid .fc-form-section {
  margin-bottom: 0;
}

.profile-info {
  display: flex;
  gap: 20px;
  align-items: flex-start;
}

.avatar-large {
  width: 80px;
  height: 80px;
  border-radius: 50%;
  background: linear-gradient(135deg, #0967d2 0%, #47a3f3 100%);
  color: white;
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: 600;
  font-size: 28px;
  flex-shrink: 0;
}

.user-details h3 {
  margin: 0 0 4px;
  font-size: 18px;
  color: #1e293b;
}

.user-details p {
  margin: 0 0 12px;
  color: #64748b;
  font-size: 14px;
}

.roles-list {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.role-tag {
  font-size: 12px;
}

.security-info {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.security-item {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 16px;
  background: #f8fafc;
  border-radius: 8px;
}

.security-icon {
  width: 40px;
  height: 40px;
  border-radius: 8px;
  background: #e2e8f0;
  display: flex;
  align-items: center;
  justify-content: center;
}

.security-icon i {
  font-size: 18px;
  color: #475569;
}

.security-details {
  flex: 1;
}

.security-details h4 {
  margin: 0 0 4px;
  font-size: 14px;
  font-weight: 500;
  color: #1e293b;
}

.security-details p {
  margin: 0;
  font-size: 13px;
  color: #64748b;
}

.sso-managed-note {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
  font-size: 13px;
  color: #64748b;
}

.cp-form {
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding-top: 8px;
}

.cp-mfa {
  border-top: 1px solid #e2e8f0;
  padding-top: 16px;
}

.cp-mfa-body {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.cp-hint {
  font-size: 12px;
  color: #64748b;
}

.cp-send {
  align-self: flex-start;
  padding-left: 0;
}

.cp-error {
  margin: 0;
  font-size: 13px;
  color: #b91c1c;
  background: #fef2f2;
  border: 1px solid #fecaca;
  border-radius: 6px;
  padding: 8px 12px;
}

.sessions-loading {
  display: flex;
  justify-content: center;
  padding: 40px;
}

.sessions-note {
  margin: 0 0 16px;
  font-size: 13px;
  color: #64748b;
}

.sessions-ip {
  font-family: monospace;
  font-size: 12px;
  color: #475569;
}

.sessions-empty {
  text-align: center;
  padding: 24px;
  color: #94a3b8;
}

.dev-cred-loading {
  display: flex;
  justify-content: center;
  padding: 16px;
}

.dev-cred-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.dev-cred-intro {
  margin: 0;
  font-size: 13px;
  color: #64748b;
}

.dev-cred-status {
  display: flex;
  align-items: center;
  gap: 10px;
}

.dev-cred-updated {
  font-size: 12px;
  color: #64748b;
}

.dev-cred-curl {
  padding: 12px;
  background: #0f172a;
  border-radius: 6px;
  overflow-x: auto;
}

.dev-cred-curl code {
  color: #e2e8f0;
  font-size: 12px;
  white-space: pre;
}

.dev-secret-dialog {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.dev-secret-warning {
  display: flex;
  align-items: center;
  gap: 8px;
  color: #f59e0b;
  font-size: 14px;
  margin: 0;
}

.dev-secret-value {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px;
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 6px;
}

.dev-secret-value code {
  flex: 1;
  font-size: 12px;
  word-break: break-all;
}
</style>
