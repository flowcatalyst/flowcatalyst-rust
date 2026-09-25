<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useConfirm } from "primevue/useconfirm";
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

// The Profile page's two-factor card: enrolled methods, adding one, recovery
// codes (authenticator-app 2FA only) and remembered devices.
const confirm = useConfirm();

const status = ref<TwoFactorStatus | null>(null);
const devices = ref<TrustedDevice[]>([]);
const loading = ref(false);
const error = ref<string | null>(null);
const success = ref<string | null>(null);
const adding = ref(false);
const newCodes = ref<string[]>([]);

const addableMethods = computed<TwoFactorMethod[]>(() => {
	const s = status.value;
	if (!s) return [];
	return s.allowedMethods.filter((m) => !s.methods.includes(m));
});

async function refresh() {
	loading.value = true;
	error.value = null;
	try {
		const [s, d] = await Promise.all([
			getTwoFactorStatus(),
			listTrustedDevices(),
		]);
		status.value = s;
		devices.value = d.devices ?? [];
	} catch (e) {
		error.value = getErrorMessage(e, "Failed to load two-factor status");
	} finally {
		loading.value = false;
	}
}

function onRemove(method: TwoFactorMethod) {
	confirm.require({
		message: `Remove ${methodLabel(method).toLowerCase()} as a second factor?`,
		header: "Remove Two-Factor Method",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Remove",
		acceptClass: "p-button-danger",
		accept: () => removeMethod(method),
	});
}

async function removeMethod(method: TwoFactorMethod) {
	error.value = null;
	success.value = null;
	try {
		await removeTwoFactorMethod(method);
		success.value = `${methodLabel(method)} removed.`;
		newCodes.value = [];
		await refresh();
	} catch (e) {
		error.value = getErrorMessage(e, "Could not remove method");
	}
}

function onRegenerate() {
	confirm.require({
		message:
			"Generate a new set of recovery codes? Your current codes stop working.",
		header: "Regenerate Recovery Codes",
		icon: "pi pi-refresh",
		acceptLabel: "Regenerate",
		accept: regenerate,
	});
}

async function regenerate() {
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
	success.value = null;
	try {
		await revokeTrustedDevice(id);
		await refresh();
	} catch (e) {
		error.value = getErrorMessage(e, "Could not revoke device");
	}
}

function startAdding() {
	error.value = null;
	success.value = null;
	adding.value = true;
}

function onAdded() {
	adding.value = false;
	success.value = "Two-factor method added.";
	void refresh();
}

function formatDate(iso?: string): string {
	if (!iso) return "never";
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

    <div v-if="loading && !status" class="loading-state">Loading…</div>

    <template v-else-if="status">
      <p class="hint">
        Add a second step to password sign-in.
        <strong v-if="status.required">Your organisation requires 2FA.</strong>
      </p>

      <!-- Enrolled methods -->
      <ul v-if="status.methods.length" class="tfa-list">
        <li v-for="m in status.methods" :key="m" class="tfa-item">
          <div class="tfa-icon">
            <i :class="m === 'TOTP' ? 'pi pi-mobile' : 'pi pi-envelope'"></i>
          </div>
          <div class="tfa-details">
            <h4>{{ methodLabel(m) }}</h4>
            <p>Enabled</p>
          </div>
          <Button
            label="Remove"
            severity="danger"
            outlined
            size="small"
            @click="onRemove(m)"
          />
        </li>
      </ul>
      <div v-else class="empty-state">
        <p>No second factor enrolled yet.</p>
      </div>

      <!-- Add a method -->
      <div v-if="adding" class="tfa-add">
        <TwoFactorSetup :allowed-methods="addableMethods" @done="onAdded" />
        <Button label="Cancel" text size="small" class="tfa-cancel" @click="adding = false" />
      </div>
      <Button
        v-else-if="addableMethods.length"
        label="Add a method"
        icon="pi pi-plus"
        outlined
        size="small"
        @click="startAdding"
      />

      <!-- Recovery codes (authenticator-app 2FA only) -->
      <div v-if="status.methods.includes('TOTP')" class="tfa-block">
        <h3 class="block-title">Recovery codes</h3>
        <p class="hint">{{ status.recoveryCodesLeft }} unused codes remaining.</p>
        <ul v-if="newCodes.length" class="tfa-recovery">
          <li v-for="c in newCodes" :key="c"><code>{{ c }}</code></li>
        </ul>
        <Button label="Regenerate recovery codes" outlined size="small" @click="onRegenerate" />
      </div>

      <!-- Remembered devices -->
      <div v-if="devices.length" class="tfa-block">
        <h3 class="block-title">Remembered devices</h3>
        <ul class="tfa-list">
          <li v-for="d in devices" :key="d.id" class="tfa-item">
            <div class="tfa-icon"><i class="pi pi-desktop"></i></div>
            <div class="tfa-details">
              <h4>{{ d.label || "Unknown device" }}</h4>
              <p>
                Last used {{ formatDate(d.lastUsedAt) }} · Expires {{ formatDate(d.expiresAt) }}
              </p>
            </div>
            <Button label="Revoke" outlined size="small" @click="onRevokeDevice(d.id)" />
          </li>
        </ul>
      </div>
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

.block-title {
  font-size: 14px;
  font-weight: 600;
  color: #243b53;
  margin: 0 0 8px;
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

.loading-state,
.empty-state {
  color: #64748b;
  font-size: 14px;
  padding: 4px 0 12px;
}

.empty-state p {
  margin: 0;
}

.tfa-list {
  list-style: none;
  padding: 0;
  margin: 0 0 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tfa-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  background: #f8fafc;
  border-radius: 8px;
}

.tfa-icon {
  width: 36px;
  height: 36px;
  border-radius: 8px;
  background: #e2e8f0;
  display: flex;
  align-items: center;
  justify-content: center;
}

.tfa-icon i {
  font-size: 16px;
  color: #475569;
}

.tfa-details {
  flex: 1;
  min-width: 0;
}

.tfa-details h4 {
  margin: 0 0 2px;
  font-size: 14px;
  font-weight: 500;
  color: #1e293b;
}

.tfa-details p {
  margin: 0;
  font-size: 12px;
  color: #64748b;
}

.tfa-add {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 16px;
  border: 1px solid #e2e8f0;
  border-radius: 8px;
}

.tfa-cancel {
  align-self: center;
}

.tfa-block {
  margin-top: 20px;
}

.tfa-recovery {
  list-style: none;
  padding: 12px 16px;
  margin: 0 0 12px;
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 8px;
  columns: 2;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}

.tfa-recovery li {
  padding: 2px 0;
}
</style>
