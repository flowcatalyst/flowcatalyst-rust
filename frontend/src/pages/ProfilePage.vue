<script setup lang="ts">
import { computed, ref } from "vue";
import { useAuthStore } from "@/stores/auth";
import {
	changePassword,
	getLoginHistory,
	sendChangePasswordEmailCode,
	type LoginHistoryItem,
} from "@/api/account";
import { toast } from "@/utils/errorBus";
import { getErrorMessage } from "@/utils/errors";
import PasskeysSection from "@/components/PasskeysSection.vue";
import TwoFactorSection from "@/components/TwoFactorSection.vue";

const authStore = useAuthStore();

// ── Change password ─────────────────────────────────────────────────────────
// The first submit goes without a code; a user with a confirmed second
// factor is answered MFA_REQUIRED (with their methods) and the dialog then
// asks for a code from one of them.
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
	if (cpBusy.value) return;
	cpError.value = "";
	if (!cpCurrent.value || !cpNew.value) {
		cpError.value = "Enter your current and new password.";
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
			// Reveal the 2FA step. With only an email factor, send the code now.
			cpMfaRequired.value = true;
			cpMethods.value = result.methods ?? [];
			if (cpHasEmailFactor.value && !cpHasTotpFactor.value) {
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

// ── Sign-in activity ────────────────────────────────────────────────────────
const showSessions = ref(false);
const sessionsLoading = ref(false);
const attempts = ref<LoginHistoryItem[]>([]);

async function openSessions() {
	showSessions.value = true;
	sessionsLoading.value = true;
	attempts.value = [];
	try {
		attempts.value = (await getLoginHistory()).attempts ?? [];
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
      <div class="fc-card">
        <h2 class="section-title">User Information</h2>
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
      </div>

      <!-- Account Settings Card -->
      <div class="fc-card">
        <h2 class="section-title">Account Settings</h2>
        <div class="settings-form">
          <div class="form-field">
            <label>Display Name</label>
            <InputText :value="authStore.displayName" disabled class="w-full" />
          </div>
          <div class="form-field">
            <label>Email</label>
            <InputText :value="authStore.user?.email" disabled class="w-full" />
          </div>
          <p v-if="authStore.user?.ssoManaged" class="sso-managed-note">
            <i class="pi pi-lock"></i>
            Your password is managed by your organisation's identity provider.
          </p>
          <div v-else class="form-actions">
            <Button
              label="Change Password"
              icon="pi pi-key"
              outlined
              @click="openChangePassword"
            />
          </div>
        </div>
      </div>

      <!-- Passkeys Card -->
      <PasskeysSection />

      <!-- Two-Factor Authentication Card -->
      <TwoFactorSection />

      <!-- Security Card -->
      <div class="fc-card">
        <h2 class="section-title">Security</h2>
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
      </div>
    </div>

    <!-- Change Password dialog -->
    <Dialog
      v-model:visible="showChangePassword"
      header="Change Password"
      modal
      :style="{ width: '28rem' }"
    >
      <div class="cp-form" @keyup.enter="submitChangePassword">
        <div class="form-field">
          <label for="cp-current">Current password</label>
          <Password
            v-model="cpCurrent"
            inputId="cp-current"
            toggleMask
            :feedback="false"
            :inputProps="{ autocomplete: 'current-password' }"
            inputClass="w-full"
            class="w-full"
          />
        </div>
        <div class="form-field">
          <label for="cp-new">New password</label>
          <Password
            v-model="cpNew"
            inputId="cp-new"
            toggleMask
            :inputProps="{ autocomplete: 'new-password' }"
            inputClass="w-full"
            class="w-full"
          />
        </div>
        <div class="form-field">
          <label for="cp-confirm">Confirm new password</label>
          <Password
            v-model="cpConfirm"
            inputId="cp-confirm"
            toggleMask
            :feedback="false"
            :inputProps="{ autocomplete: 'new-password' }"
            inputClass="w-full"
            class="w-full"
          />
        </div>

        <div v-if="cpMfaRequired" class="form-field">
          <label for="cp-code">Two-factor code</label>
          <InputText
            id="cp-code"
            v-model="cpCode"
            placeholder="Enter your code"
            autocomplete="one-time-code"
            class="w-full"
          />
          <small v-if="cpHasTotpFactor" class="cp-hint">
            Enter the code from your authenticator app{{ cpHasEmailFactor ? ", or send an email code below." : "." }}
          </small>
          <small v-else-if="cpHasEmailFactor" class="cp-hint">We sent a code to your email.</small>
          <Button
            v-if="cpHasEmailFactor"
            type="button"
            label="Send email code"
            icon="pi pi-envelope"
            text
            size="small"
            class="cp-send"
            @click="sendEmailCode"
          />
        </div>

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
        <DataTable :value="attempts" size="small" stripedRows>
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
              <span class="font-mono">{{ data.ipAddress || '—' }}</span>
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

.section-title {
  font-size: 16px;
  font-weight: 600;
  color: #243b53;
  margin: 0 0 20px;
  padding-bottom: 12px;
  border-bottom: 1px solid #e2e8f0;
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

.settings-form {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.form-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.form-field label {
  font-size: 14px;
  font-weight: 500;
  color: #334e68;
}

.form-actions {
  margin-top: 8px;
}

.sso-managed-note {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 8px 0 0;
  font-size: 13px;
  color: #64748b;
}

.cp-form {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.cp-hint {
  font-size: 12px;
  color: #64748b;
}

.cp-send {
  align-self: flex-start;
}

.cp-error {
  margin: 0;
  font-size: 14px;
  color: #dc2626;
}

.sessions-loading {
  display: flex;
  justify-content: center;
  padding: 24px 0;
}

.sessions-note {
  margin: 0 0 12px;
  font-size: 13px;
  color: #64748b;
  line-height: 1.5;
}

.sessions-empty {
  padding: 12px 0;
  color: #64748b;
  font-size: 14px;
}

.font-mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 13px;
}

:deep(.p-password) {
  width: 100%;
}

:deep(.p-password-input) {
  width: 100%;
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
</style>
