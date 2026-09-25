<script setup lang="ts">
import { onMounted, ref } from "vue";
import { toast } from "@/utils/errorBus";
import {
	resetApprovalsApi,
	type ResetApprovalRequest,
} from "@/api/reset-approvals";
import { getErrorMessage } from "@/utils/errors";

const requests = ref<ResetApprovalRequest[]>([]);
const loading = ref(false);
const error = ref<string | null>(null);
const busyId = ref<string | null>(null);

async function refresh() {
	loading.value = true;
	error.value = null;
	try {
		requests.value = (await resetApprovalsApi.list()).requests;
	} catch (e) {
		error.value = getErrorMessage(e, "Failed to load reset requests");
	} finally {
		loading.value = false;
	}
}

async function approve(r: ResetApprovalRequest) {
	busyId.value = r.id;
	try {
		await resetApprovalsApi.approve(r.id);
		toast.success("Approved", `${r.email} has been emailed a reset link.`);
		await refresh();
	} catch (e) {
		toast.error("Error", getErrorMessage(e, "Could not approve"));
	} finally {
		busyId.value = null;
	}
}

async function deny(r: ResetApprovalRequest) {
	busyId.value = r.id;
	try {
		await resetApprovalsApi.deny(r.id);
		toast.success("Denied", "Request denied.");
		await refresh();
	} catch (e) {
		toast.error("Error", getErrorMessage(e, "Could not deny"));
	} finally {
		busyId.value = null;
	}
}

function formatDate(iso: string): string {
	return new Date(iso).toLocaleString(undefined, {
		year: "numeric",
		month: "short",
		day: "numeric",
		hour: "2-digit",
		minute: "2-digit",
	});
}

onMounted(refresh);
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Password reset approvals</h1>
        <p class="page-subtitle">
          Users with no authenticator or passkey who've requested a password
          reset. Approving emails them a reset link and clears their 2FA so they
          re-enrol.
        </p>
      </div>
      <Button icon="pi pi-refresh" text @click="refresh" :loading="loading" />
    </header>

    <Message v-if="error" severity="error" :closable="true" @close="error = null">
      {{ error }}
    </Message>

    <div class="fc-card">
      <div v-if="loading" class="empty">Loading…</div>
      <div v-else-if="!requests.length" class="empty">
        No pending reset requests.
      </div>
      <table v-else class="ra-table">
        <thead>
          <tr>
            <th>User</th>
            <th>Requested</th>
            <th>Expires</th>
            <th class="actions-col"></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="r in requests" :key="r.id">
            <td>
              <div class="ra-user">
                <span class="ra-name">{{ r.name || r.email }}</span>
                <span class="ra-email">{{ r.email }}</span>
              </div>
            </td>
            <td>{{ formatDate(r.createdAt) }}</td>
            <td>{{ formatDate(r.expiresAt) }}</td>
            <td class="actions-col">
              <Button
                label="Approve"
                size="small"
                :loading="busyId === r.id"
                @click="approve(r)"
              />
              <Button
                label="Deny"
                size="small"
                severity="secondary"
                text
                :disabled="busyId === r.id"
                @click="deny(r)"
              />
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<style scoped>
.page-header {
	display: flex;
	justify-content: space-between;
	align-items: flex-start;
	gap: 1rem;
}
.ra-table {
	width: 100%;
	border-collapse: collapse;
}
.ra-table th,
.ra-table td {
	text-align: left;
	padding: 0.6rem 0.75rem;
	border-bottom: 1px solid var(--surface-200, #e5e7eb);
}
.ra-user {
	display: flex;
	flex-direction: column;
}
.ra-name {
	font-weight: 500;
}
.ra-email {
	font-size: 0.85rem;
	color: var(--text-color-secondary, #6b7280);
}
.actions-col {
	text-align: right;
	white-space: nowrap;
}
.empty {
	padding: 1.5rem;
	text-align: center;
	color: var(--text-color-secondary, #6b7280);
}
</style>
