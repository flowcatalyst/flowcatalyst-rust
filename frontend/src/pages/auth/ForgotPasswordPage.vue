<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useRoute } from "vue-router";
import {
	normalizeClientParam,
	useLoginThemeStore,
} from "@/stores/loginTheme";
import { oauthAuthorizeUrl, requestPasswordReset } from "@/api/auth";
import { getErrorMessage } from "@/utils/errors";

const route = useRoute();
const themeStore = useLoginThemeStore();

onMounted(async () => {
	await themeStore.loadTheme(normalizeClientParam(route.query["client"]));
	themeStore.applyThemeColors();

	// Pre-populate email from query param (set by LoginPage)
	const emailParam = route.query["email"];
	if (emailParam && typeof emailParam === "string") {
		email.value = emailParam;
	}
});

const email = ref("");
const isSubmitting = ref(false);
const submitted = ref(false);
const errorMessage = ref<string | null>(null);

async function onSubmit() {
	if (!email.value.trim() || isSubmitting.value) return;

	isSubmitting.value = true;
	errorMessage.value = null;

	try {
		// Arrived here from the login page mid-OAuth round-trip (the login
		// page forwards its ?oauth=true&client_id=… query): ask the server to
		// bring the user back to that authorize URL after the reset, so the
		// sign-in to the other application completes instead of being lost.
		const redirectUri =
			route.query["oauth"] === "true"
				? oauthAuthorizeUrl((field) => {
						const value = route.query[field];
						return typeof value === "string" ? value : null;
					})
				: undefined;
		await requestPasswordReset(email.value.trim(), redirectUri);
		submitted.value = true;
	} catch (e: unknown) {
		errorMessage.value = getErrorMessage(e, "Something went wrong. Please try again.");
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
        <!-- Success state -->
        <template v-if="submitted">
          <h2 class="login-title">Check your email</h2>
          <div class="success-message">
            <p>
              If an account exists for <strong>{{ email }}</strong>, you'll receive
              a password reset link shortly.
            </p>
          </div>
          <RouterLink :to="{ name: 'login' }" class="back-link">
            Back to sign in
          </RouterLink>
        </template>

        <!-- Form state -->
        <template v-else>
          <h2 class="login-title">Reset your password</h2>
          <p class="form-description">
            Enter your email address and we'll send you a link to reset your password.
          </p>

          <div v-if="errorMessage" class="error-message">
            <p>{{ errorMessage }}</p>
          </div>

          <form class="login-form" @submit.prevent="onSubmit">
            <div class="form-field">
              <label for="email">Email address</label>
              <InputText
                id="email"
                v-model="email"
                type="email"
                placeholder="you@company.com"
                :disabled="isSubmitting"
                class="w-full"
              />
            </div>

            <Button
              type="submit"
              label="Send reset link"
              :loading="isSubmitting"
              :disabled="!email.trim()"
              class="w-full"
            />
          </form>

          <div class="back-row">
            <RouterLink :to="{ name: 'login' }" class="back-link">
              Back to sign in
            </RouterLink>
          </div>
        </template>
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
  margin: 0 0 12px;
}

.form-description {
  color: #627d98;
  font-size: 14px;
  margin: 0 0 24px;
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

.success-message {
  background: #f0fdf4;
  border: 1px solid #bbf7d0;
  border-radius: 8px;
  padding: 16px;
  margin-bottom: 24px;
}

.success-message p {
  margin: 0;
  color: #166534;
  font-size: 14px;
  line-height: 1.6;
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

.back-row {
  margin-top: 20px;
  text-align: center;
}

.back-link {
  font-size: 14px;
  color: var(--login-accent, #0967d2);
  text-decoration: none;
  display: inline-block;
}

.back-link:hover {
  color: #0552b5;
}

.login-footer {
  text-align: center;
  color: #627d98;
  font-size: 14px;
  margin: 24px 0 0;
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
