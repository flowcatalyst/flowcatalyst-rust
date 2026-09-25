<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
	getTwoFactorStatus,
	listTrustedDevices,
	methodLabel,
	regenerateRecoveryCodes,
	removeTwoFactorMethod,
	revokeTrustedDevice,
	type TrustedDevice,
	type TwoFactorMethod,
	type TwoFactorStatus,
} from "@/api/twofactor";
import { getErrorMessage } from "@/utils/errors";
import TwoFactorSetup from "@/components/TwoFactorSetup.vue";

const status = ref<TwoFactorStatus | null>(null);
const devices = ref<TrustedDevice[]>([]);
const loading = ref(false);
const error = ref<string | null>(null);
const success = ref<string | null>(null);
const adding = ref(false);
const newCodes = ref<string[]>([]);

const addableMethods = computed<TwoFactorMethod[]>(() => {
	if (!status.value) return [];
	return status.value.allowedMethods.filter(
		(m) => !status.value!.methods.includes(m),
	);
});

async function refresh() {
	loading.value = true;
	error.value = null;
	try {
		status.value = await getTwoFactorStatus();
		devices.value = (await listTrustedDevices()).devices;
	} catch (e) {
		error.value = getErrorMessage(e, "Failed to load two-factor status");
	} finally {
		loading.value = false;
	}
}

async function onRemove(method: TwoFactorMethod) {
	error.value = null;
	success.value = null;
	try {
		await removeTwoFactorMethod(method);
		success.value = `${methodLabel(method)} removed.`;
		await refresh();
	} catch (e) {
		error.value = getErrorMessage(e, "Could not remove method");
	}
}

async function onRegenerate() {
	error.value = null;
	success.value = null;
	try {
		newCodes.value = (await regenerateRecoveryCodes()).recoveryCodes;
		success.value = "New recovery codes generated. Save them now.";
		await refresh();
	} catch (e) {
		error.value = getErrorMessage(e, "Could not regenerate codes");
	}
}

async function onRevokeDevice(id: string) {
	error.value = null;
	try {
		await revokeTrustedDevice(id);
		await refresh();
	} catch (e) {
		error.value = getErrorMessage(e, "Could not revoke device");
	}
}

function onAdded() {
	adding.value = false;
	success.value = "Two-factor method added.";
	void refresh();
}

function formatDate(iso?: string): string {
	if (!iso) return "—";
	return new Date(iso).toLocaleDateString(undefined, {
		year: "numeric",
		month: "short",
		day: "numeric",
	});
}

onMounted(refresh);
</script>

<template>
  <div class="fc-card">
    <h2 class="section-title">Two-Factor Authentication</h2>

    <div v-if="error" class="error-banner">{{ error }}</div>
    <div v-if="success" class="success-banner">{{ success }}</div>

    <div v-if="loading" class="loading-state">Loading…</div>

    <template v-else-if="status">
      <p class="hint">
        Add a second step to password sign-in.
        <strong v-if="status.required">Your organisation requires 2FA.</strong>
      </p>

      <!-- Enrolled methods -->
      <div v-if="status.methods.length" class="tfa-list">
        <div v-for="m in status.methods" :key="m" class="tfa-row">
          <span><i class="pi pi-check-circle" /> {{ methodLabel(m) }}</span>
          <Button
            label="Remove"
            severity="danger"
            text
            size="small"
            @click="onRemove(m)"
          />
        </div>
      </div>
      <p v-else class="hint muted">No second factor enrolled yet.</p>

      <!-- Add a method -->
      <div v-if="adding" class="tfa-add">
        <TwoFactorSetup :allowed-methods="addableMethods" @done="onAdded" />
        <Button label="Cancel" text size="small" @click="adding = false" />
      </div>
      <Button
        v-else-if="addableMethods.length"
        label="Add a method"
        icon="pi pi-plus"
        @click="adding = true"
      />

      <!-- Recovery codes (authenticator-app 2FA only) -->
      <div v-if="status.methods.includes('TOTP')" class="tfa-recovery-block">
        <h3>Recovery codes</h3>
        <p class="hint">{{ status.recoveryCodesLeft }} unused codes remaining.</p>
        <ul v-if="newCodes.length" class="tfa-recovery">
          <li v-for="c in newCodes" :key="c"><code>{{ c }}</code></li>
        </ul>
        <Button label="Regenerate recovery codes" outlined size="small" @click="onRegenerate" />
      </div>

      <!-- Trusted devices -->
      <div v-if="devices.length" class="tfa-devices">
        <h3>Remembered devices</h3>
        <div v-for="d in devices" :key="d.id" class="tfa-row">
          <span>
            {{ d.label || "Unknown device" }}
            <small class="muted">· last used {{ formatDate(d.lastUsedAt) }}</small>
          </span>
          <Button label="Revoke" text size="small" @click="onRevokeDevice(d.id)" />
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.tfa-list,
.tfa-devices,
.tfa-add {
	margin: 0.75rem 0;
}
.tfa-row {
	display: flex;
	align-items: center;
	justify-content: space-between;
	padding: 0.4rem 0;
	border-bottom: 1px solid var(--surface-200, #e5e7eb);
}
.tfa-recovery-block,
.tfa-devices {
	margin-top: 1.25rem;
}
.tfa-recovery {
	list-style: none;
	padding: 0.75rem 1rem;
	margin: 0.5rem 0;
	background: var(--surface-100, #f3f4f6);
	border-radius: 6px;
	columns: 2;
}
.muted {
	color: var(--text-color-secondary, #6b7280);
}
</style>
