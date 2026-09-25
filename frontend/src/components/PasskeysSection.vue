<script setup lang="ts">
import { onMounted, ref } from "vue";
import {
	type CredentialSummary,
	isPlatformAuthenticatorAvailable,
	isWebauthnSupported,
	listPasskeys,
	registerPasskey,
	revokePasskey,
} from "@/api/webauthn";
import { getErrorMessage } from "@/utils/errors";

const passkeys = ref<CredentialSummary[]>([]);
const loading = ref(false);
const registering = ref(false);
const newPasskeyName = ref("");
const error = ref<string | null>(null);
const successMessage = ref<string | null>(null);
const supported = ref(false);
const platformAvailable = ref(false);

async function refresh() {
	loading.value = true;
	error.value = null;
	try {
		passkeys.value = await listPasskeys();
	} catch (e) {
		error.value = getErrorMessage(e, "Failed to load passkeys");
	} finally {
		loading.value = false;
	}
}

async function onRegister() {
	if (registering.value) return;
	const name = newPasskeyName.value.trim();
	if (!name) {
		error.value = "Please name this passkey before adding it.";
		return;
	}
	registering.value = true;
	error.value = null;
	successMessage.value = null;
	try {
		await registerPasskey(name);
		successMessage.value = "Passkey added.";
		newPasskeyName.value = "";
		await refresh();
	} catch (e) {
		// User cancelled in the authenticator UI is the most common case;
		// surface friendlier messages for the known authenticator errors.
		// Match on the DOMException NAME — the message carries the human
		// description, not the name, so message matching never fired.
		const name = e instanceof Error ? e.name : "";
		const message = getErrorMessage(e, "Failed to add passkey");
		if (name === "NotAllowedError") {
			error.value = "Cancelled — no passkey was added.";
		} else if (
			name === "InvalidStateError" ||
			message.toLowerCase().includes("previously registered")
		) {
			error.value =
				"This device already has a passkey for your account — use a different device or security key to add another.";
		} else {
			error.value = message;
		}
	} finally {
		registering.value = false;
	}
}

async function onRevoke(credentialId: string) {
	error.value = null;
	successMessage.value = null;
	try {
		await revokePasskey(credentialId);
		successMessage.value = "Passkey removed.";
		await refresh();
	} catch (e) {
		error.value = getErrorMessage(e, "Failed to remove passkey");
	}
}

function formatDate(iso: string | null): string {
	if (!iso) return "—";
	return new Date(iso).toLocaleDateString(undefined, {
		year: "numeric",
		month: "short",
		day: "numeric",
	});
}

onMounted(async () => {
	supported.value = isWebauthnSupported();
	if (!supported.value) return;
	platformAvailable.value = await isPlatformAuthenticatorAvailable();
	await refresh();
});
</script>

<template>
  <div class="fc-card">
    <h2 class="section-title">Passkeys</h2>

    <p v-if="!supported" class="hint">
      This browser doesn't support passkeys. Use a recent version of Chrome,
      Safari, Firefox, or Edge over HTTPS.
    </p>

    <template v-else>
      <p class="hint">
        Passkeys let you sign in without a password using Touch ID, Face ID,
        Windows Hello, or a hardware security key. They replace passwords for
        most sign-in flows.
      </p>

      <div v-if="error" class="error-banner">{{ error }}</div>
      <div v-if="successMessage" class="success-banner">{{ successMessage }}</div>

      <div class="add-passkey">
        <InputText
          v-model="newPasskeyName"
          placeholder="Name (e.g. 'My Computer/Phone')"
          :disabled="registering"
          class="passkey-name-input"
        />
        <Button
          label="Add a passkey"
          icon="pi pi-key"
          :loading="registering"
          :disabled="!newPasskeyName.trim()"
          @click="onRegister"
        />
      </div>

      <div v-if="loading" class="loading-state">Loading…</div>
      <div v-else-if="passkeys.length === 0" class="empty-state">
        <p>No passkeys registered yet.</p>
      </div>
      <ul v-else class="passkey-list">
        <li v-for="key in passkeys" :key="key.id" class="passkey-item">
          <div class="passkey-icon"><i class="pi pi-key"></i></div>
          <div class="passkey-details">
            <h4>{{ key.name || "Unnamed passkey" }}</h4>
            <p>
              Added {{ formatDate(key.createdAt) }}
              <template v-if="key.lastUsedAt">
                · Last used {{ formatDate(key.lastUsedAt) }}
              </template>
            </p>
          </div>
          <Button
            label="Remove"
            severity="danger"
            outlined
            size="small"
            @click="onRevoke(key.id)"
          />
        </li>
      </ul>
    </template>
  </div>
</template>

<style scoped>
.section-title {
	font-size: 16px;
	font-weight: 600;
	color: #243b53;
	margin: 0 0 16px;
	padding-bottom: 12px;
	border-bottom: 1px solid #e2e8f0;
}

.hint {
	font-size: 13px;
	color: #64748b;
	margin: 0 0 16px;
	line-height: 1.5;
}

.error-banner {
	background: #fef2f2;
	border: 1px solid #fecaca;
	color: #dc2626;
	border-radius: 8px;
	padding: 10px 14px;
	font-size: 14px;
	margin-bottom: 12px;
}

.success-banner {
	background: #f0fdf4;
	border: 1px solid #bbf7d0;
	color: #166534;
	border-radius: 8px;
	padding: 10px 14px;
	font-size: 14px;
	margin-bottom: 12px;
}

.add-passkey {
	display: flex;
	gap: 8px;
	align-items: stretch;
	margin-bottom: 16px;
}

.passkey-name-input {
	flex: 1;
}

.loading-state,
.empty-state {
	color: #64748b;
	font-size: 14px;
	padding: 12px 0;
}

.passkey-list {
	list-style: none;
	padding: 0;
	margin: 0;
	display: flex;
	flex-direction: column;
	gap: 8px;
}

.passkey-item {
	display: flex;
	align-items: center;
	gap: 12px;
	padding: 12px 16px;
	background: #f8fafc;
	border-radius: 8px;
}

.passkey-icon {
	width: 36px;
	height: 36px;
	border-radius: 8px;
	background: #e2e8f0;
	display: flex;
	align-items: center;
	justify-content: center;
}

.passkey-icon i {
	font-size: 16px;
	color: #475569;
}

.passkey-details {
	flex: 1;
	min-width: 0;
}

.passkey-details h4 {
	margin: 0 0 2px;
	font-size: 14px;
	font-weight: 500;
	color: #1e293b;
}

.passkey-details p {
	margin: 0;
	font-size: 12px;
	color: #64748b;
}
</style>
