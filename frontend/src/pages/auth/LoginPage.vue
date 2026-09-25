<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import { useForm, useField } from "vee-validate";
import { toTypedSchema } from "@vee-validate/zod";
import { z } from "zod";
import { useAuthStore } from "@/stores/auth";
import {
	normalizeClientParam,
	useLoginThemeStore,
} from "@/stores/loginTheme";
import {
	checkEmailDomain,
	checkSession,
	externalIdpRedirectUrl,
	login,
	oauthAuthorizeUrl,
	redirectAfterLogin,
	requestPasswordSetup,
	type LoginResult,
} from "@/api/auth";
import { authenticateWithPasskey, isWebauthnSupported } from "@/api/webauthn";
import TwoFactorChallenge from "@/components/TwoFactorChallenge.vue";
import TwoFactorSetup from "@/components/TwoFactorSetup.vue";
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
	await themeStore.loadTheme(normalizeClientParam(route.query["client"]));
	themeStore.applyThemeColors();
});

const step = ref<LoginStep>("email");
const isSubmitting = ref(false);
const mfaChallenge = ref<MfaChallenge | null>(null);
const mfaEnroll = ref<MfaEnroll | null>(null);

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
	authStore.setError(null);
}

const onCheckEmail = handleEmailSubmit(async (values) => {
	isSubmitting.value = true;
	authStore.setError(null);

	try {
		const result = await checkEmailDomain(values.email);

		if (result.authMethod === "external" && result.loginUrl) {
			step.value = "redirecting";
			// Forward OIDC-interaction / OAuth round-trip context to the IdP
			// login URL (shared helper — see api/auth.ts).
			window.location.href = externalIdpRedirectUrl(result.loginUrl);
		} else if (result.authMethod === "internal" && result.passwordSetupRequired) {
			// App-created internal user who has never set a password: we
			// never accept a new password inline — email them a
			// mailbox-proving set-password link instead.
			step.value = "setup";
		} else {
			step.value = "password";
		}
	} catch (e: unknown) {
		// Surface the failure — a silent catch here meant "Continue" did
		// nothing at all when the domain check errored.
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
		// "ok" → login() already established the session and redirected.
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
		// Only forward a redirect when the page was entered as an OAuth
		// round-trip (?oauth=true) — the rebuilt /oauth/authorize?... URL is
		// what sends the user back into the calling application once they've
		// set their password. Same field list / getter shape as every other
		// oauthAuthorizeUrl call site (see api/auth.ts, router/guards.ts).
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
		// Silent-success endpoint — an error here means the request itself
		// failed (network/5xx), not that the account doesn't exist.
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
		await authenticateWithPasskey(currentEmail.value);
		// The server set the session cookie. Load the FULL session (permissions,
		// clientId, roles) into the store so the sidebar and landing decision
		// have real data immediately — a partial setUser here previously left the
		// store with clientId:null and no permissions, so the nav rendered empty
		// until a manual page reload.
		await checkSession();
		// Same post-login navigation as the password path (OIDC interaction /
		// OAuth round-trip / landing page).
		redirectAfterLogin();
	} catch (e) {
		// DOMException name, not message: getErrorMessage returns the human
		// description, which doesn't contain the error name — the friendly
		// "Cancelled" branch never fired on message matching.
		authStore.setError(
			e instanceof Error && e.name === "NotAllowedError"
				? "Cancelled — try again or use your password."
				: getErrorMessage(e, "Passkey sign-in failed"),
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
        <h2 class="login-title">
          {{
            step === 'email'
              ? 'Sign in to your account'
              : step === 'password'
                ? 'Enter your password'
                : step === 'setup'
                  ? 'Create your password'
                  : step === 'setupSent'
                    ? 'Check your email'
                    : step === '2fa'
                      ? 'Verify it\'s you'
                      : step === 'enroll'
                        ? 'Set up two-factor authentication'
                        : 'Redirecting...'
          }}
        </h2>

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
              autocomplete="username"
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
              :inputProps="{ autocomplete: 'current-password' }"
              inputClass="w-full"
              class="w-full"
              @blur="passwordTouched = true"
            />
          </div>

          <div class="form-options">
            <!-- The whole query goes along: mid-OAuth (?oauth=true&client_id=…)
                 the forgot page needs it to bring the user back to the
                 application after the reset. -->
            <RouterLink
              :to="{ name: 'forgot-password', query: { ...route.query, ...(currentEmail ? { email: currentEmail } : {}) } }"
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

        <!-- Password setup step: internal user created by an application,
             invite email suppressed — first time signing in, no password
             set yet. We never accept a new password inline here; the
             emailed link is what proves mailbox ownership. -->
        <div v-if="step === 'setup'" class="login-form">
          <div class="email-display">
            <div class="email-info">
              <div class="email-avatar">
                {{ currentEmail.charAt(0).toUpperCase() }}
              </div>
              <span class="email-text">{{ currentEmail }}</span>
            </div>
            <button type="button" class="change-email-btn" @click="onChangeEmail">
              Use a different email
            </button>
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

        <!-- Password setup link sent -->
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

        <!-- 2FA challenge step -->
        <TwoFactorChallenge
          v-if="step === '2fa' && mfaChallenge"
          :mfa-token="mfaChallenge.mfaToken"
          :methods="mfaChallenge.methods"
          :remember-device-allowed="mfaChallenge.rememberDeviceAllowed"
        />

        <!-- Forced enrollment step -->
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
