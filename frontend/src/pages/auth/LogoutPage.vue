<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useLoginThemeStore } from "@/stores/loginTheme";
import { logout } from "@/api/auth";

const themeStore = useLoginThemeStore();
const isLoggingOut = ref(false);

onMounted(async () => {
	await themeStore.loadTheme();
	themeStore.applyThemeColors();
});

async function onConfirmLogout() {
	if (isLoggingOut.value) return;
	isLoggingOut.value = true;
	// `logout()` POSTs /auth/logout, clears local state, and redirects to /auth/login.
	await logout();
}
</script>

<template>
	<div class="logout-container" :style="{ background: themeStore.background }">
		<div class="logout-content">
			<div class="logout-header">
				<h1 class="brand-name">{{ themeStore.theme.brandName }}</h1>
				<p class="brand-subtitle">{{ themeStore.theme.brandSubtitle }}</p>
			</div>

			<div class="logout-card">
				<h2 class="logout-title">Log out of the Identity Server?</h2>
				<p class="logout-description">
					You have been signed out of the application. To finish signing out
					completely — for example so that you can sign back in as a different
					user — you also need to log out of the Identity Server.
				</p>

				<button
					type="button"
					class="logout-button"
					:disabled="isLoggingOut"
					@click="onConfirmLogout"
				>
					<span v-if="isLoggingOut" class="spinner-inline" aria-hidden="true" />
					{{ isLoggingOut ? "Logging out…" : "Log out of Identity Server" }}
				</button>

				<p class="logout-hint">
					If you don't want to log out, you can simply close this tab.
				</p>
			</div>
		</div>
	</div>
</template>

<style scoped>
.logout-container {
	min-height: 100vh;
	display: flex;
	align-items: center;
	justify-content: center;
	padding: 24px;
	font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
}

.logout-content {
	width: 100%;
	max-width: 440px;
}

.logout-header {
	text-align: center;
	margin-bottom: 32px;
	color: white;
}

.brand-name {
	font-size: 28px;
	font-weight: 700;
	margin: 0 0 4px;
}

.brand-subtitle {
	font-size: 14px;
	opacity: 0.85;
	margin: 0;
}

.logout-card {
	background: white;
	border-radius: 12px;
	padding: 40px 32px;
	box-shadow: 0 20px 60px rgba(0, 0, 0, 0.2);
}

.logout-title {
	font-size: 22px;
	font-weight: 600;
	color: #102a43;
	margin: 0 0 16px;
	text-align: center;
}

.logout-description {
	font-size: 14px;
	line-height: 1.6;
	color: #486581;
	margin: 0 0 28px;
	text-align: center;
}

.logout-button {
	width: 100%;
	padding: 14px 16px;
	font-size: 15px;
	font-weight: 600;
	color: white;
	background: var(--login-accent, #0967d2);
	border: none;
	border-radius: 8px;
	cursor: pointer;
	display: flex;
	align-items: center;
	justify-content: center;
	gap: 10px;
	transition: opacity 0.15s;
}

.logout-button:hover:not(:disabled) {
	opacity: 0.92;
}

.logout-button:disabled {
	opacity: 0.6;
	cursor: not-allowed;
}

.logout-hint {
	font-size: 12px;
	color: #829ab1;
	margin: 20px 0 0;
	text-align: center;
}

.spinner-inline {
	width: 16px;
	height: 16px;
	border: 2px solid rgba(255, 255, 255, 0.4);
	border-top-color: white;
	border-radius: 50%;
	animation: spin 0.8s linear infinite;
}

@keyframes spin {
	to {
		transform: rotate(360deg);
	}
}
</style>
