<script setup lang="ts">
// Portal-plane login (docs/portal-identity-plan.md Phase 2.5 v2): reached
// via GET /portal/authorize?flow=… — a portal app's OAuth entry. Portal
// identities are a separate population from platform users, so this page
// has NO guest guard and never touches the platform session: every visit
// authenticates fresh and ends in a redirect back to the portal app.
import { ref, computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import { useLoginThemeStore } from "@/stores/loginTheme";

const route = useRoute();
const themeStore = useLoginThemeStore();

const flowId = ref<string>("");
const email = ref("");
const password = ref("");
// step: "email" → check-domain → "password" (or SSO redirect) → done
const step = ref<"email" | "password" | "expired">("email");
const submitting = ref(false);
const error = ref<string | null>(null);

const emailValid = computed(() => /.+@.+\..+/.test(email.value.trim()));

onMounted(async () => {
	document.title = "Portal Login";
	await themeStore.loadTheme();
	themeStore.applyThemeColors();
	const flow = route.query["flow"];
	if (typeof flow === "string" && flow !== "") {
		flowId.value = flow;
	} else {
		step.value = "expired";
	}
});

interface PortalError {
	code?: string;
	message?: string;
}

async function portalPost<T>(path: string, body: unknown): Promise<T> {
	const res = await fetch(path, {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify(body),
	});
	const data = (await res.json().catch(() => ({}))) as T & PortalError;
	if (!res.ok) {
		if (data.code === "FLOW_EXPIRED") {
			step.value = "expired";
		}
		throw new Error(data.message || "Request failed");
	}
	return data;
}

async function continueWithEmail() {
	if (!emailValid.value || submitting.value) return;
	submitting.value = true;
	error.value = null;
	try {
		const result = await portalPost<{ method: string; redirectUrl?: string }>(
			"/portal/auth/check-domain",
			{ flowId: flowId.value, email: email.value.trim() },
		);
		if (result.method === "SSO" && result.redirectUrl) {
			// Org-federated: hand off to the organisation's identity provider.
			window.location.assign(result.redirectUrl);
			return;
		}
		step.value = "password";
	} catch (e) {
		if (step.value !== "expired") {
			error.value = e instanceof Error ? e.message : "Something went wrong";
		}
	} finally {
		submitting.value = false;
	}
}

function changeEmail() {
	step.value = "email";
	password.value = "";
	error.value = null;
	resetSent.value = false;
}

const resetSent = ref(false);
const resetSending = ref(false);

async function forgotPassword() {
	if (resetSending.value || resetSent.value) return;
	resetSending.value = true;
	error.value = null;
	try {
		await portalPost<{ message: string }>("/portal/auth/password-reset", {
			flowId: flowId.value,
			email: email.value.trim(),
		});
		resetSent.value = true;
	} catch (e) {
		if (step.value !== "expired") {
			error.value = e instanceof Error ? e.message : "Something went wrong";
		}
	} finally {
		resetSending.value = false;
	}
}

async function signIn() {
	if (!password.value || submitting.value) return;
	submitting.value = true;
	error.value = null;
	try {
		const result = await portalPost<{ redirectUrl: string }>(
			"/portal/auth/login",
			{ flowId: flowId.value, email: email.value.trim(), password: password.value },
		);
		window.location.assign(result.redirectUrl);
	} catch (e) {
		if (step.value !== "expired") {
			error.value = e instanceof Error ? e.message : "Sign-in failed";
		}
	} finally {
		submitting.value = false;
	}
}
</script>

<template>
  <div class="login-container" :style="{ background: themeStore.background }">
    <div class="login-content">
      <!-- Deliberately NO platform branding: the portal plane serves a
           client's customers, not platform users. -->
      <div class="login-header">
        <h1 class="brand-name">Portal Login</h1>
      </div>

      <!-- Card -->
      <div class="login-card">
        <!-- Expired / missing flow -->
        <template v-if="step === 'expired'">
          <h2 class="login-title">Sign-in link expired</h2>
          <p class="portal-message">
            This sign-in session has expired. Please return to the application
            and try again — it will bring you back here.
          </p>
        </template>

        <template v-else>
          <h2 class="login-title">Sign in</h2>
          <p class="login-subtitle">
            {{ step === 'email'
              ? "Enter your email address to continue."
              : "Enter your password to sign in." }}
          </p>

          <div v-if="error" class="error-message">
            <p>{{ error }}</p>
          </div>

          <!-- Step 1: email -->
          <form v-if="step === 'email'" class="login-form" @submit.prevent="continueWithEmail">
            <div class="form-field">
              <label for="portalEmail">Email</label>
              <InputText
                id="portalEmail"
                v-model="email"
                type="email"
                autocomplete="username"
                placeholder="you@company.com"
                autofocus
                fluid
              />
            </div>
            <Button
              type="submit"
              label="Continue"
              class="login-button"
              :loading="submitting"
              :disabled="!emailValid"
            />
          </form>

          <!-- Step 2: password -->
          <form v-else class="login-form" @submit.prevent="signIn">
            <div class="identity-row">
              <span class="identity-email">{{ email }}</span>
              <button type="button" class="action-link identity-change" @click="changeEmail">
                Change
              </button>
            </div>
            <div class="form-field">
              <label for="portalPassword">Password</label>
              <!-- autocomplete must ride inputProps: PrimeVue Password sets
                   inheritAttrs:false, so a plain attr never reaches the
                   inner <input>. -->
              <Password
                id="portalPassword"
                v-model="password"
                :feedback="false"
                toggle-mask
                :inputProps="{ autocomplete: 'current-password' }"
                placeholder="Your password"
                autofocus
                fluid
              />
            </div>
            <Button
              type="submit"
              label="Sign in"
              class="login-button"
              :loading="submitting"
              :disabled="!password"
            />
            <div class="forgot-row">
              <span v-if="resetSent" class="reset-sent">
                If an account exists, a reset email has been sent.
              </span>
              <button
                v-else
                type="button"
                class="action-link"
                :disabled="resetSending"
                @click="forgotPassword"
              >
                Forgot password?
              </button>
            </div>
          </form>
        </template>
      </div>


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

.brand-name {
  font-size: 32px;
  font-weight: 700;
  color: white;
  margin: 0;
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
  margin: 0 0 8px;
}

.login-subtitle {
  color: #64748b;
  font-size: 14px;
  margin: 0 0 24px;
}

.portal-message {
  color: #334e68;
  font-size: 14px;
  line-height: 1.6;
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

.identity-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  background: #f0f4f8;
  border-radius: 8px;
  padding: 10px 14px;
}

.identity-email {
  font-size: 14px;
  color: #102a43;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.identity-change {
  background: none;
  border: none;
  padding: 0;
  cursor: pointer;
  flex-shrink: 0;
}

.login-button {
  width: 100%;
}

.forgot-row {
  text-align: center;
  margin-top: -8px;
}

.forgot-row .action-link {
  background: none;
  border: none;
  padding: 0;
  cursor: pointer;
}

.reset-sent {
  font-size: 14px;
  color: #486581;
}

.action-link {
  display: inline-block;
  font-size: 14px;
  color: var(--login-accent, #0967d2);
  text-decoration: none;
}

.action-link:hover {
  color: #0552b5;
}

</style>
