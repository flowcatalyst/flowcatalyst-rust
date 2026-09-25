<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useAuthStore } from "@/stores/auth";
import { getErrorMessage } from "@/utils/errors";

const router = useRouter();
const authStore = useAuthStore();
const error = ref<string | null>(null);

onMounted(async () => {
	const params = new URLSearchParams(window.location.search);
	const code = params.get("code");
	const state = params.get("state");
	const errorParam = params.get("error");
	const errorDescription = params.get("error_description");

	if (errorParam) {
		error.value = errorDescription || errorParam;
		return;
	}

	if (!code) {
		error.value = "No authorization code received";
		return;
	}

	// Verify state
	const savedState = sessionStorage.getItem("oauth_state");
	if (state !== savedState) {
		error.value = "Invalid state parameter";
		return;
	}

	try {
		// Exchange code for tokens
		const response = await fetch("/oauth/token", {
			method: "POST",
			headers: {
				"Content-Type": "application/x-www-form-urlencoded",
			},
			body: new URLSearchParams({
				grant_type: "authorization_code",
				code: code,
				redirect_uri: `${window.location.origin}/auth/callback`,
				client_id: "platform-admin-ui",
			}),
			credentials: "include",
		});

		if (!response.ok) {
			const data = await response.json().catch(() => ({}));
			throw new Error(
				data.error_description || data.error || "Token exchange failed",
			);
		}

		// Token exchange successful - session cookie should be set
		// Now fetch user info
		const meResponse = await fetch("/auth/me", {
			credentials: "include",
		});

		if (!meResponse.ok) {
			throw new Error("Failed to fetch user info");
		}

		const userData = await meResponse.json();
		authStore.setUser({
			id: userData.principalId,
			email: userData.email,
			name: userData.name,
			clientId: userData.clientId,
			roles: Array.from(userData.roles || []),
			permissions: [],
			ssoManaged: userData.ssoManaged ?? false,
		});

		// Redirect to intended destination
		const redirectPath =
			sessionStorage.getItem("auth_redirect") || "/dashboard";
		sessionStorage.removeItem("auth_redirect");
		sessionStorage.removeItem("oauth_state");

		await router.replace(redirectPath);
	} catch (e: unknown) {
		error.value = getErrorMessage(e, "Authentication failed");
	}
});
</script>

<template>
  <div class="callback-container">
    <div v-if="error" class="error-state">
      <i class="pi pi-exclamation-circle error-icon"></i>
      <h2>Authentication Failed</h2>
      <p>{{ error }}</p>
      <a href="/auth/login" class="retry-link">Try again</a>
    </div>
    <div v-else class="loading-state">
      <ProgressSpinner strokeWidth="3" />
      <p>Completing authentication...</p>
    </div>
  </div>
</template>

<style scoped>
.callback-container {
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  background: linear-gradient(135deg, #102a43 0%, #0a1929 100%);
}

.loading-state,
.error-state {
  text-align: center;
  color: white;
}

.loading-state p,
.error-state p {
  margin-top: 16px;
  color: #9fb3c8;
}

.error-state {
  background: rgba(255, 255, 255, 0.1);
  padding: 40px;
  border-radius: 16px;
}

.error-icon {
  font-size: 48px;
  color: #f87171;
}

.error-state h2 {
  margin: 16px 0 8px;
}

.retry-link {
  display: inline-block;
  margin-top: 24px;
  padding: 12px 24px;
  background: #0967d2;
  color: white;
  text-decoration: none;
  border-radius: 8px;
}

.retry-link:hover {
  background: #0552b5;
}
</style>
