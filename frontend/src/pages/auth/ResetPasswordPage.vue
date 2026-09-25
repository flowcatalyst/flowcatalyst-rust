<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useForm, useField } from "vee-validate";
import { toTypedSchema } from "@vee-validate/zod";
import { z } from "zod";
import {
	normalizeClientParam,
	useLoginThemeStore,
} from "@/stores/loginTheme";
import { useAuthStore } from "@/stores/auth";
import { landingPath } from "@/stores/permissions";
import {
	validateResetToken,
	confirmPasswordReset,
	setPostAuthRedirect,
	checkSession,
} from "@/api/auth";

// Invite framing: the same page serves first-time invites (route
// "set-password" — incl. portal identities) and self-service resets.
import TwoFactorSetup from "@/components/TwoFactorSetup.vue";
import type { TwoFactorMethod } from "@/api/twofactor";
import { getErrorMessage } from "@/utils/errors";
import { passwordPolicyError } from "@/utils/passwordPolicy";

const route = useRoute();
const router = useRouter();
const themeStore = useLoginThemeStore();
const authStore = useAuthStore();

onMounted(async () => {
	await themeStore.loadTheme(normalizeClientParam(route.query["client"]));
	themeStore.applyThemeColors();
	await checkToken();
});

type PageState =
	| "loading"
	| "invalid"
	| "form"
	| "submitting"
	| "enroll"
	| "portalDone"
	| "redirecting";

const pageState = ref<PageState>("loading");
const isInvite = computed(() => route.name === "set-password");
// Portal-identity token (from validate): no platform branding anywhere.
// While loading on the invite route we also stay neutral so a portal
// invitee never sees a flash of platform identity.
const isPortal = ref(false);
const showPlatformBrand = computed(
	() => !isPortal.value && !(isInvite.value && pageState.value === "loading"),
);
const invalidReason = ref<"expired" | "not_found" | "unknown">("not_found");
const submitError = ref<string | null>(null);
const enrollToken = ref("");
const enrollMethods = ref<TwoFactorMethod[]>([]);
const requiresFactor = ref(false);
const factorCode = ref("");

const token = (route.query["token"] as string | undefined) ?? "";

async function checkToken() {
	if (!token) {
		invalidReason.value = "not_found";
		pageState.value = "invalid";
		return;
	}

	try {
		const result = await validateResetToken(token);
		isPortal.value = result.portal ?? false;
		if (isPortal.value) document.title = "Set your password";
		if (result.valid) {
			requiresFactor.value = result.requiresFactor ?? false;
			pageState.value = "form";
		} else {
			invalidReason.value =
				result.reason === "expired"
					? "expired"
					: result.reason === "not_found"
						? "not_found"
						: "unknown";
			pageState.value = "invalid";
		}
	} catch {
		invalidReason.value = "unknown";
		pageState.value = "invalid";
	}
}

// Password schema — mirrors the server policy (passwordPolicyError): length
// bounds + not-a-common-password. The server additionally rejects passwords
// derived from the account's email/name (unknown to this page — the reset
// token doesn't expose it); that error surfaces via submitError on confirm.
const passwordSchema = toTypedSchema(
	z
		.object({
			password: z.string().superRefine((pw, ctx) => {
				const err = passwordPolicyError(pw);
				if (err) ctx.addIssue({ code: z.ZodIssueCode.custom, message: err });
			}),
			confirmPassword: z.string(),
		})
		.refine((data) => data.password === data.confirmPassword, {
			message: "Passwords do not match",
			path: ["confirmPassword"],
		}),
);

const { handleSubmit } = useForm({ validationSchema: passwordSchema });

const { value: passwordValue, errorMessage: passwordError } =
	useField<string>("password");
const { value: confirmPasswordValue, errorMessage: confirmPasswordError } =
	useField<string>("confirmPassword");

const onSubmit = handleSubmit(async (values) => {
	if (requiresFactor.value && !factorCode.value.trim()) {
		submitError.value = "Enter the code from your authenticator app.";
		return;
	}
	pageState.value = "submitting";
	submitError.value = null;

	try {
		const result = await confirmPasswordReset(
			token,
			values.password,
			requiresFactor.value ? factorCode.value.trim() : undefined,
		);
		if (result.status === "enrollment_required" && result.enrollToken) {
			// Domain requires 2FA — set it up before finishing. TwoFactorSetup
			// completes the session and redirects on its own; an invite's
			// server-validated redirect is stashed for it to follow.
			if (result.redirectUri) setPostAuthRedirect(result.redirectUri);
			enrollToken.value = result.enrollToken;
			enrollMethods.value = result.allowedMethods ?? [];
			pageState.value = "enroll";
			return;
		}
		if (result.redirectUri) {
			// Invite redirect (a portal's OAuth login, or the application that
			// created the user via inviteRedirectUri) — validated server-side
			// when the invite was minted. A platform session, when one was
			// just established, lets that application's sign-in go straight
			// through.
			window.location.assign(result.redirectUri);
			return;
		}
		if (result.sessionEstablished) {
			// Platform invite, no redirect stashed, no 2FA required: the
			// server already set the session cookie. Load the full session
			// (permissions, clientId, roles) so landingPath resolves
			// correctly, then go straight in — there's no login page to
			// bounce back to in this flow.
			pageState.value = "redirecting";
			await checkSession();
			await router.replace(landingPath(authStore.user));
			return;
		}
		if (result.portal) {
			// A portal identity with no redirect must NOT land on the
			// platform login — it cannot sign portal users in.
			pageState.value = "portalDone";
			return;
		}
		await router.replace({ name: "login", query: { reset: "success" } });
	} catch (e: unknown) {
		submitError.value = getErrorMessage(
			e,
			"Failed to reset password. Please try again.",
		);
		pageState.value = "form";
	}
});
</script>

<template>
  <div class="login-container" :style="{ background: themeStore.background }">
    <div class="login-content">
      <!-- Logo and branding (suppressed for portal identities) -->
      <div v-if="!showPlatformBrand" class="login-header">
        <h1 class="brand-name">Portal</h1>
      </div>
      <div v-else class="login-header">
        <img
          v-if="themeStore.theme.logoUrl"
          :src="themeStore.theme.logoUrl"
          class="logo-image"
          alt="Logo"
        />
        <div
          v-else-if="themeStore.theme.logoSvg"
          class="logo-svg"
          v-html="themeStore.theme.logoSvg"
        />
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

      <!-- Card -->
      <div class="login-card">
        <!-- Loading state -->
        <div v-if="pageState === 'loading'" class="loading-state">
          <div class="spinner"></div>
          <p>{{ isInvite ? "Validating your invite link..." : "Validating your reset link..." }}</p>
        </div>

        <!-- Invalid / expired token -->
        <template v-else-if="pageState === 'invalid'">
          <h2 class="login-title">Link invalid or expired</h2>
          <div class="error-message">
            <p v-if="invalidReason === 'expired' && isInvite">
              This invite link has expired. Ask your administrator to send a new invite.
            </p>
            <p v-else-if="invalidReason === 'expired'">
              This password reset link has expired. Reset links are valid for 15 minutes.
            </p>
            <p v-else-if="isInvite">
              This invite link is invalid or has already been used.
            </p>
            <p v-else>
              This password reset link is invalid or has already been used.
            </p>
          </div>
          <RouterLink
            v-if="!isInvite"
            :to="{ name: 'forgot-password' }"
            class="action-link"
          >
            Request a new reset link
          </RouterLink>
        </template>

        <!-- Forced 2FA enrollment after the password is set -->
        <template v-else-if="pageState === 'enroll'">
          <h2 class="login-title">Set up two-factor authentication</h2>
          <TwoFactorSetup :enroll-token="enrollToken" :allowed-methods="enrollMethods" />
        </template>

        <!-- Session already established server-side (invite, no 2FA) -->
        <template v-else-if="pageState === 'redirecting'">
          <h2 class="login-title">Password set</h2>
          <div class="loading-state">
            <div class="spinner"></div>
            <p>Your password is set. Taking you to your account...</p>
          </div>
        </template>

        <!-- Portal identity finished with no redirect configured -->
        <template v-else-if="pageState === 'portalDone'">
          <h2 class="login-title">Password set</h2>
          <p class="portal-done-message">
            Your password has been set. Return to your portal to sign in.
          </p>
        </template>

        <!-- Set/reset form -->
        <template v-else>
          <h2 class="login-title">{{ isInvite ? "Set your password" : "Set a new password" }}</h2>

          <div v-if="submitError" class="error-message">
            <p>{{ submitError }}</p>
          </div>

          <form class="login-form" @submit.prevent="onSubmit">
            <div class="form-field">
              <label for="password">{{ isInvite ? "Password" : "New password" }}</label>
              <!-- PrimeVue Password drops plain attrs (inheritAttrs:false) —
                   autocomplete must go through inputProps to reach the input,
                   or Chrome mis-classifies the field and autofills/reveals
                   stored values instead of offering a new password. -->
              <Password
                id="password"
                v-model="passwordValue"
                placeholder="At least 8 characters"
                :disabled="pageState === 'submitting'"
                :invalid="!!passwordError"
                :feedback="true"
                toggleMask
                :inputProps="{ autocomplete: 'new-password' }"
                inputClass="w-full"
                class="w-full"
              />
              <small v-if="passwordError" class="field-error">{{ passwordError }}</small>
            </div>

            <div class="form-field">
              <label for="confirmPassword">{{ isInvite ? "Confirm password" : "Confirm new password" }}</label>
              <Password
                id="confirmPassword"
                v-model="confirmPasswordValue"
                :placeholder="isInvite ? 'Repeat your password' : 'Repeat your new password'"
                :disabled="pageState === 'submitting'"
                :invalid="!!confirmPasswordError"
                :feedback="false"
                toggleMask
                :inputProps="{ autocomplete: 'new-password' }"
                inputClass="w-full"
                class="w-full"
              />
              <small v-if="confirmPasswordError" class="field-error">
                {{ confirmPasswordError }}
              </small>
            </div>

            <!-- Strong-factor gate: a self-service reset needs an authenticator
                 code (email alone can't authorize it). -->
            <div v-if="requiresFactor" class="form-field">
              <label for="factorCode">Authenticator code</label>
              <InputText
                id="factorCode"
                v-model="factorCode"
                placeholder="123456"
                inputmode="numeric"
                autocomplete="one-time-code"
                :disabled="pageState === 'submitting'"
                class="w-full"
              />
              <small class="field-hint">
                Enter the 6-digit code from your authenticator app to confirm.
              </small>
            </div>

            <Button
              type="submit"
              :label="isInvite ? 'Set password' : 'Reset password'"
              :loading="pageState === 'submitting'"
              class="w-full"
            />
          </form>
        </template>
      </div>

      <!-- Footer -->
      <p v-if="showPlatformBrand" class="login-footer">
        {{ themeStore.theme.footerText }}
      </p>
    </div>
  </div>
</template>

<style scoped>
.portal-done-message {
	margin: 0.5rem 0 0;
	line-height: 1.5;
}

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

.loading-state {
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
  to { transform: rotate(360deg); }
}

.loading-state p {
  color: #64748b;
  margin: 0;
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

.field-error {
  color: #dc2626;
  font-size: 12px;
}

.action-link {
  display: inline-block;
  font-size: 14px;
  color: var(--login-accent, #0967d2);
  text-decoration: none;
  margin-top: 4px;
}

.action-link:hover {
  color: #0552b5;
}

.login-footer {
  text-align: center;
  color: #627d98;
  font-size: 14px;
  margin: 24px 0 0;
}

:deep(.p-password) {
  width: 100%;
}

:deep(.p-password-input) {
  width: 100%;
}

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
