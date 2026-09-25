<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import { useForm, useField } from "vee-validate";
import { toTypedSchema } from "@vee-validate/zod";
import { z } from "zod";
import { useAuthStore } from "@/stores/auth";
import { useLoginThemeStore } from "@/stores/loginTheme";
import {
	checkEmailDomain,
	loadPermissions,
	login,
	oauthAuthorizeUrl,
	requestPasswordSetup,
	type LoginResult,
} from "@/api/auth";
import { authenticateWithPasskey, isWebauthnSupported } from "@/api/webauthn";
import TwoFactorChallenge from "@/components/TwoFactorChallenge.vue";
import TwoFactorSetup from "@/components/TwoFactorSetup.vue";
import router from "@/router";
import { getErrorMessage } from "@/utils/errors";

type LoginStep =
	| "email"
	| "password"
	| "setup"
	| "setupSent"
	| "redirecting"
	| "2fa"
	| "enroll";

type MfaChallenge = Extract<LoginResult, { status: "mfa_required" }>;
type MfaEnroll = Extract<LoginResult, { status: "enrollment_required" }>;

const route = useRoute();
const authStore = useAuthStore();
const themeStore = useLoginThemeStore();

// Show a success banner when redirected here after a successful password reset
const showResetSuccess = computed(() => route.query["reset"] === "success");

// Load theme on mount
onMounted(async () => {
	await themeStore.loadTheme();
	themeStore.applyThemeColors();
});

const step = ref<LoginStep>("email");
const isSubmitting = ref(false);
// The pending second step of a password sign-in (no session yet).
const mfaChallenge = ref<MfaChallenge | null>(null);
const mfaEnroll = ref<MfaEnroll | null>(null);

const stepTitle = computed(() => {
	switch (step.value) {
		case "email":
			return "Sign in to your account";
		case "password":
			return "Enter your password";
		case "setup":
			return "Create your password";
		case "setupSent":
			return "Check your email";
		case "2fa":
			return "Verify it's you";
		case "enroll":
			return "Set up two-factor authentication";
		default:
			return "Redirecting...";
	}
});

// Email step schema
const emailSchema = toTypedSchema(
	z.object({
		email: z
			.string()
			.min(1, "Email is required")
			.email("Please enter a valid email address"),
	}),
);

// Email form
const {
	handleSubmit: handleEmailSubmit,
	values: _emailValues,
	meta: _emailMeta,
} = useForm({
	validationSchema: emailSchema,
	initialValues: { email: "" },
});

const { value: emailValue, errorMessage: emailError } =
	useField<string>("email");

// Password form - separate form context
const passwordValue = ref("");
const passwordTouched = ref(false);

const isEmailValid = computed(() => {
	// Simple validation - has content and looks like an email
	const email = emailValue.value || "";
	return email.length > 0 && email.includes("@") && email.includes(".");
});

const isPasswordValid = computed(() => {
	return passwordValue.value.length > 0;
});

const currentEmail = computed(() => emailValue.value || "");

function onChangeEmail() {
	step.value = "email";
	passwordValue.value = "";
	passwordTouched.value = false;
	mfaChallenge.value = null;
	mfaEnroll.value = null;
	authStore.setError(null);
}

const onCheckEmail = handleEmailSubmit(async (values) => {
	isSubmitting.value = true;
	authStore.setError(null);

	try {
		const result = await checkEmailDomain(values.email);

		if (result.authMethod === "external" && result.loginUrl) {
			step.value = "redirecting";

			// Forward OAuth params to OIDC login if this is part of an OAuth flow
			const currentParams = new URLSearchParams(window.location.search);
			let redirectUrl = result.loginUrl;

			// Forward interaction param for OIDC interaction flow
			const interactionUid = currentParams.get("interaction");
			if (interactionUid) {
				const loginUrl = new URL(result.loginUrl, window.location.origin);
				loginUrl.searchParams.set("interaction", interactionUid);
				redirectUrl = loginUrl.toString();
			} else if (currentParams.get("oauth") === "true") {
				const oauthFields = [
					"client_id",
					"redirect_uri",
					"scope",
					"state",
					"code_challenge",
					"code_challenge_method",
					"nonce",
				];
				const loginUrl = new URL(result.loginUrl, window.location.origin);

				for (const field of oauthFields) {
					const value = currentParams.get(field);
					if (value) {
						// Map to oauth_ prefix expected by /auth/oidc/login
						loginUrl.searchParams.set("oauth_" + field, value);
					}
				}
				redirectUrl = loginUrl.toString();
			}

			window.location.href = redirectUrl;
		} else if (result.authMethod === "internal" && result.passwordSetupRequired) {
			// A password account that has never had a password (e.g. created
			// by an application with its invite suppressed). A new password is
			// never accepted inline — the emailed link proves the mailbox.
			step.value = "setup";
		} else {
			step.value = "password";
		}
	} catch (e: unknown) {
		authStore.setError(
			getErrorMessage(e, "Could not check your email — please try again."),
		);
	} finally {
		isSubmitting.value = false;
	}
});

async function onSubmitPassword() {
	if (!isPasswordValid.value || isSubmitting.value) return;

	isSubmitting.value = true;

	try {
		const result = await login({
			email: currentEmail.value,
			password: passwordValue.value,
		});
		if (result.status === "mfa_required") {
			mfaChallenge.value = result;
			step.value = "2fa";
		} else if (result.status === "enrollment_required") {
			mfaEnroll.value = result;
			step.value = "enroll";
		}
		// "ok": login() has set the session and navigated.
	} catch {
		// Error is handled by AuthStore
	} finally {
		isSubmitting.value = false;
	}
}

async function onRequestPasswordSetup() {
	if (isSubmitting.value) return;

	isSubmitting.value = true;
	authStore.setError(null);

	try {
		// Mid-OAuth (?oauth=true) the rebuilt /oauth/authorize URL rides along
		// so the user returns to the calling application afterwards.
		const redirectUri =
			route.query["oauth"] === "true"
				? oauthAuthorizeUrl((field) => {
						const value = route.query[field];
						return typeof value === "string" ? value : null;
					})
				: undefined;
		await requestPasswordSetup(currentEmail.value, redirectUri);
		step.value = "setupSent";
	} catch (e: unknown) {
		// A silent-success endpoint: an error is the request itself failing.
		authStore.setError(
			getErrorMessage(e, "Could not send the email — please try again."),
		);
	} finally {
		isSubmitting.value = false;
	}
}

const passkeySupported = computed(() => isWebauthnSupported());

async function onPasskeyLogin() {
	if (!currentEmail.value || isSubmitting.value) return;

	isSubmitting.value = true;
	authStore.setError(null);

	try {
		const result = await authenticateWithPasskey(currentEmail.value);
		// Server set the session cookie. Mirror what /auth/login does to
		// keep the auth store in sync and honour OAuth/OIDC redirects.
		authStore.setUser({
			id: result.principalId,
			email: result.email ?? currentEmail.value,
			name: result.name,
			clientId: null,
			roles: result.roles,
			permissions: null,
		});
		await loadPermissions();

		const urlParams = new URLSearchParams(window.location.search);
		const interactionUid = urlParams.get("interaction");
		if (interactionUid) {
			window.location.href = `/oidc/interaction/${interactionUid}/login`;
			return;
		}
		if (urlParams.get("oauth") === "true") {
			const oauthFields = [
				"response_type",
				"client_id",
				"redirect_uri",
				"scope",
				"state",
				"code_challenge",
				"code_challenge_method",
				"nonce",
			];
			const oauthParams = new URLSearchParams();
			for (const field of oauthFields) {
				const value = urlParams.get(field);
				if (value) oauthParams.set(field, value);
			}
			window.location.href = `/oauth/authorize?${oauthParams.toString()}`;
			return;
		}
		await router.replace("/dashboard");
	} catch (e) {
		const message = getErrorMessage(e, "Passkey sign-in failed");
		authStore.setError(
			message.includes("NotAllowedError")
				? "Cancelled — try again or use your password."
				: message,
		);
	} finally {
		isSubmitting.value = false;
	}
}
</script>

<template>
  <div class="login-container" :style="{ background: themeStore.background }">
    <div class="login-content">
      <!-- Logo and branding -->
      <div class="login-header">
        <!-- Custom logo URL -->
        <img
          v-if="themeStore.theme.logoUrl"
          :src="themeStore.theme.logoUrl"
          class="logo-image"
          alt="Logo"
        />
        <!-- Custom logo SVG -->
        <div
          v-else-if="themeStore.theme.logoSvg"
          class="logo-svg"
          v-html="themeStore.theme.logoSvg"
        />
        <!-- Default logo -->
        <div v-else class="logo-container">
          <svg class="logo-icon" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="1.5"
              d="M13 10V3L4 14h7v7l9-11h-7z"
            />
          </svg>
        </div>
        <h1 class="brand-name">{{ themeStore.theme.brandName }}</h1>
        <p class="brand-subtitle">{{ themeStore.theme.brandSubtitle }}</p>
      </div>

      <!-- Login card -->
      <div class="login-card">
        <h2 class="login-title">{{ stepTitle }}</h2>

        <!-- Password reset success banner -->
        <div v-if="showResetSuccess" class="success-banner">
          <p>Your password has been reset. You can now sign in with your new password.</p>
        </div>

        <!-- Error message -->
        <div v-if="authStore.error" class="error-message">
          <p>{{ authStore.error }}</p>
        </div>

        <!-- Redirecting state -->
        <div v-if="step === 'redirecting'" class="redirecting-state">
          <div class="spinner"></div>
          <p>Redirecting to your organization's login...</p>
        </div>

        <!-- Email step -->
        <form v-if="step === 'email'" class="login-form" @submit.prevent="onCheckEmail">
          <div class="form-field">
            <label for="email">Email address</label>
            <InputText
              id="email"
              v-model="emailValue"
              type="email"
              placeholder="you@company.com"
              :disabled="isSubmitting"
              :invalid="!!emailError"
              class="w-full"
            />
            <small v-if="emailError" class="field-error">{{ emailError }}</small>
            <small v-else class="field-hint"
              >We'll check if your organization uses single sign-on</small
            >
          </div>

          <Button
            type="submit"
            label="Continue"
            :loading="isSubmitting"
            :disabled="!isEmailValid"
            class="w-full"
          />
        </form>

        <!-- Password step -->
        <form v-if="step === 'password'" class="login-form" @submit.prevent="onSubmitPassword">
          <!-- Show email (read-only) with change option -->
          <div class="email-display">
            <div class="email-info">
              <div class="email-avatar">
                {{ currentEmail.charAt(0).toUpperCase() }}
              </div>
              <span class="email-text">{{ currentEmail }}</span>
            </div>
            <button type="button" class="change-email-btn" @click="onChangeEmail">Change</button>
          </div>

          <div class="form-field">
            <label for="password">Password</label>
            <Password
              id="password"
              v-model="passwordValue"
              placeholder="Enter your password"
              :disabled="isSubmitting"
              :feedback="false"
              toggleMask
              inputClass="w-full"
              class="w-full"
              @blur="passwordTouched = true"
            />
          </div>

          <div class="form-options">
            <RouterLink
              :to="{ name: 'forgot-password', query: currentEmail ? { email: currentEmail } : {} }"
              class="forgot-password"
            >Forgot password?</RouterLink>
          </div>

          <Button
            type="submit"
            label="Sign in"
            :loading="isSubmitting"
            :disabled="!isPasswordValid"
            class="w-full"
          />

          <!-- Passkey alternative — server will silently 401 if the user
               has no passkey or their domain is federated, so it's safe
               to show whenever WebAuthn is supported. -->
          <div v-if="passkeySupported" class="passkey-divider">or</div>
          <Button
            v-if="passkeySupported"
            type="button"
            label="Sign in with a passkey"
            icon="pi pi-key"
            severity="secondary"
            outlined
            :disabled="isSubmitting"
            class="w-full passkey-button"
            @click="onPasskeyLogin"
          />
        </form>

        <!-- Create-your-password step: an internal user who has never set a
             password. The emailed link proves mailbox ownership. -->
        <div v-if="step === 'setup'" class="login-form">
          <div class="email-display">
            <div class="email-info">
              <div class="email-avatar">
                {{ currentEmail.charAt(0).toUpperCase() }}
              </div>
              <span class="email-text">{{ currentEmail }}</span>
            </div>
            <button type="button" class="change-email-btn" @click="onChangeEmail">Change</button>
          </div>

          <p class="form-description">
            This is your first time signing in. We'll email you a link to
            create your password — this confirms it's really you.
          </p>

          <Button
            type="button"
            label="Email me a link"
            :loading="isSubmitting"
            class="w-full"
            @click="onRequestPasswordSetup"
          />
        </div>

        <!-- Create-your-password link sent -->
        <div v-if="step === 'setupSent'" class="login-form">
          <div class="success-banner">
            <p>
              We sent a link to <strong>{{ currentEmail }}</strong>. Open it
              on this device to create your password. The link expires in 72
              hours.
            </p>
          </div>
          <button type="button" class="change-email-btn" @click="onChangeEmail">
            Back to sign in
          </button>
        </div>

        <!-- Two-factor challenge -->
        <div v-if="step === '2fa' && mfaChallenge" class="login-form">
          <TwoFactorChallenge
            :mfa-token="mfaChallenge.mfaToken"
            :methods="mfaChallenge.methods"
            :remember-device-allowed="mfaChallenge.rememberDeviceAllowed"
          />
          <button type="button" class="change-email-btn" @click="onChangeEmail">
            Back to sign in
          </button>
        </div>

        <!-- Two-factor enrolment the email domain requires. Confirming signs
             the user in (then shows the recovery codes once), so there is no
             way back to the email step from here. -->
        <TwoFactorSetup
          v-if="step === 'enroll' && mfaEnroll"
          :enroll-token="mfaEnroll.enrollToken"
          :allowed-methods="mfaEnroll.allowedMethods"
        />
      </div>

      <!-- Footer -->
      <p class="login-footer">
        {{ themeStore.theme.footerText }}
      </p>
    </div>
  </div>
</template>

<style scoped>
.login-container {
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--login-bg, linear-gradient(135deg, #102a43 0%, #0a1929 100%));
  padding: 16px;
}

.login-content {
  width: 100%;
  max-width: 480px;
}

.login-header {
  text-align: center;
  margin-bottom: 32px;
}

.logo-container {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 72px;
  height: 72px;
  background: rgba(255, 255, 255, 0.1);
  border-radius: 16px;
  margin-bottom: 16px;
}

.logo-icon {
  width: 40px;
  height: 40px;
  color: white;
}

.logo-image {
  max-width: 200px;
  max-height: 72px;
  margin-bottom: 16px;
  object-fit: contain;
}

.logo-svg {
  margin-bottom: 16px;
}

.logo-svg :deep(svg) {
  max-width: 200px;
  max-height: 72px;
}

.brand-name {
  font-size: 32px;
  font-weight: 700;
  color: white;
  margin: 0;
}

.brand-subtitle {
  color: #9fb3c8;
  margin: 8px 0 0;
  font-size: 16px;
}

.login-card {
  background: white;
  border-radius: 16px;
  padding: 40px;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.3);
}

.login-title {
  font-size: 20px;
  font-weight: 600;
  color: #102a43;
  margin: 0 0 24px;
}

.success-banner {
  background: #f0fdf4;
  border: 1px solid #bbf7d0;
  border-radius: 8px;
  padding: 12px 16px;
  margin-bottom: 24px;
}

.success-banner p {
  margin: 0;
  color: #166534;
  font-size: 14px;
}

.error-message {
  background: #fef2f2;
  border: 1px solid #fecaca;
  border-radius: 8px;
  padding: 12px 16px;
  margin-bottom: 24px;
}

.error-message p {
  margin: 0;
  color: #dc2626;
  font-size: 14px;
}

.redirecting-state {
  text-align: center;
  padding: 32px 0;
}

.spinner {
  width: 32px;
  height: 32px;
  border: 4px solid #e2e8f0;
  border-top-color: var(--login-accent, #0967d2);
  border-radius: 50%;
  animation: spin 1s linear infinite;
  margin: 0 auto 16px;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.redirecting-state p {
  color: #64748b;
  margin: 0;
}

.login-form {
  display: flex;
  flex-direction: column;
  gap: 24px;
}

.form-field {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.form-field label {
  font-size: 14px;
  font-weight: 500;
  color: #334e68;
}

.form-description {
  color: #627d98;
  font-size: 14px;
  margin: 0;
  line-height: 1.6;
}

.field-hint {
  color: #627d98;
  font-size: 12px;
}

.field-error {
  color: #dc2626;
  font-size: 12px;
}

.email-display {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  background: #f8fafc;
  border-radius: 8px;
}

.email-info {
  display: flex;
  align-items: center;
  gap: 12px;
}

.email-avatar {
  width: 40px;
  height: 40px;
  background: linear-gradient(135deg, var(--login-accent, #0967d2) 0%, #47a3f3 100%);
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: white;
  font-weight: 600;
}

.email-text {
  color: #475569;
  font-size: 14px;
}

.change-email-btn {
  background: none;
  border: none;
  color: var(--login-accent, #0967d2);
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
}

.change-email-btn:hover {
  color: #0552b5;
}

.form-options {
  display: flex;
  justify-content: flex-end;
}

.forgot-password {
  font-size: 14px;
  color: var(--login-accent, #0967d2);
  text-decoration: none;
}

.forgot-password:hover {
  color: #0552b5;
}

.login-footer {
  text-align: center;
  color: #627d98;
  font-size: 14px;
  margin: 24px 0 0;
}

.passkey-divider {
  text-align: center;
  font-size: 12px;
  color: #94a3b8;
  position: relative;
  margin: 4px 0;
}

.passkey-divider::before,
.passkey-divider::after {
  content: "";
  position: absolute;
  top: 50%;
  width: calc(50% - 20px);
  height: 1px;
  background: #e2e8f0;
}

.passkey-divider::before {
  left: 0;
}

.passkey-divider::after {
  right: 0;
}

/* Don't override the secondary outlined passkey button with the accent color. */
:deep(.passkey-button.p-button) {
  background: transparent;
  color: #475569;
  border-color: #cbd5e1;
}

:deep(.passkey-button.p-button:not(:disabled):hover) {
  background: #f1f5f9;
  border-color: #94a3b8;
  color: #1e293b;
}

/* Override PrimeVue Password component width */
:deep(.p-password) {
  width: 100%;
}

:deep(.p-password-input) {
  width: 100%;
}

/* Override PrimeVue Button to use theme accent color */
:deep(.p-button) {
  background: var(--login-accent, #0967d2);
  border-color: var(--login-accent, #0967d2);
}

:deep(.p-button:not(:disabled):hover) {
  background: color-mix(in srgb, var(--login-accent, #0967d2) 85%, black);
  border-color: color-mix(in srgb, var(--login-accent, #0967d2) 85%, black);
}

:deep(.p-button:not(:disabled):active) {
  background: color-mix(in srgb, var(--login-accent, #0967d2) 75%, black);
  border-color: color-mix(in srgb, var(--login-accent, #0967d2) 75%, black);
}

:deep(.p-button:focus-visible) {
  outline-color: var(--login-accent, #0967d2);
  box-shadow:
    0 0 0 2px #ffffff,
    0 0 0 4px color-mix(in srgb, var(--login-accent, #0967d2) 50%, transparent);
}

:deep(.p-button:disabled) {
  background: color-mix(in srgb, var(--login-accent, #0967d2) 50%, #e2e8f0);
  border-color: color-mix(in srgb, var(--login-accent, #0967d2) 50%, #e2e8f0);
}
</style>
