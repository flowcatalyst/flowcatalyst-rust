<script setup lang="ts">
// Portal Users — per-client administration of the portal identity plane
// (docs/portal-identity-plan.md Phase 2.5 v2). Visible to platform admins
// (any client, via the picker) and to client administrators holding the
// platform:portal-administrator role (their own client(s)).
//
// There is deliberately no invite here: invites are initiated by the
// portal app itself (POST /api/portal-users with its portalAppCode), which
// owns the relationship with its users. This page searches, suspends,
// reactivates, revokes per-app access, and offboards.
import { ref, computed, onMounted, watch } from "vue";
import { useConfirm } from "primevue/useconfirm";
import type { DataTablePageEvent } from "primevue/datatable";
import { toast } from "@/utils/errorBus";
import {
	portalUsersApi,
	type PortalUser,
	type PortalUserApp,
	type PortalUserState,
} from "@/api/portal-users";
import { portalAppsApi, type PortalApp } from "@/api/portal-apps";
import { clientsApi, type Client } from "@/api/clients";
import { useAuthStore } from "@/stores/auth";
import { getErrorMessage } from "@/utils/errors";

const confirm = useConfirm();
const authStore = useAuthStore();

const clients = ref<Client[]>([]);
const selectedClientId = ref<string>("");
const apps = ref<PortalApp[]>([]);
const selectedAppCode = ref<string>("");
const search = ref("");
const portalUsers = ref<PortalUser[]>([]);
const total = ref(0);
const page = ref(0);
const pageSize = ref(25);
const loading = ref(false);

const isAnchor = computed(() => !authStore.user?.clientId);

const clientOptions = computed(() => {
	if (isAnchor.value) {
		return clients.value.map((c) => ({ label: c.name, value: c.id }));
	}
	return authStore.accessibleClients.map((id) => ({
		label: clients.value.find((c) => c.id === id)?.name || id,
		value: id,
	}));
});

// Sentinel filter value: users granted no portal app at all.
const UNASSIGNED = "__unassigned__";
const appOptions = computed(() => [
	{ label: "No portal app (unassigned)", value: UNASSIGNED },
	...apps.value.map((a) => ({ label: `${a.name} (${a.code})`, value: a.code })),
]);

onMounted(async () => {
	try {
		const response = await clientsApi.list();
		clients.value = response.clients || [];
	} catch {
		// Client admins may not list clients; the picker falls back to ids.
	}
	// Client admins land on their own client; anchors pick one.
	if (!isAnchor.value) {
		selectedClientId.value =
			authStore.accessibleClients[0] ?? authStore.user?.clientId ?? "";
	} else if (clientOptions.value.length === 1) {
		selectedClientId.value = clientOptions.value[0]?.value ?? "";
	}
});

watch(selectedClientId, async () => {
	selectedAppCode.value = "";
	apps.value = [];
	page.value = 0;
	if (!selectedClientId.value) return;
	try {
		apps.value = (await portalAppsApi.list(selectedClientId.value)).portalApps;
	} catch (e: unknown) {
		toast.error("Error", getErrorMessage(e, "Failed to load portal apps"));
	}
	void loadPortalUsers();
});

watch(selectedAppCode, () => {
	page.value = 0;
	void loadPortalUsers();
});

// Server-side prefix search (TERM% on email and name), debounced.
let searchTimer: ReturnType<typeof setTimeout> | undefined;
watch(search, () => {
	clearTimeout(searchTimer);
	searchTimer = setTimeout(() => {
		page.value = 0;
		void loadPortalUsers();
	}, 300);
});

// Drop out-of-order responses (a slow search landing after a newer one).
let requestSeq = 0;

async function loadPortalUsers() {
	if (!selectedClientId.value) return;
	const seq = ++requestSeq;
	loading.value = true;
	try {
		const response = await portalUsersApi.list({
			clientId: selectedClientId.value,
			q: search.value.trim() || undefined,
			portalAppCode:
				selectedAppCode.value && selectedAppCode.value !== UNASSIGNED
					? selectedAppCode.value
					: undefined,
			unassigned: selectedAppCode.value === UNASSIGNED || undefined,
			page: page.value,
			size: pageSize.value,
		});
		if (seq !== requestSeq) return;
		portalUsers.value = response.portalUsers;
		total.value = response.total;
	} catch (e: unknown) {
		if (seq === requestSeq) {
			toast.error("Error", getErrorMessage(e, "Failed to load portal users"));
		}
	} finally {
		if (seq === requestSeq) loading.value = false;
	}
}

function onPage(event: DataTablePageEvent) {
	page.value = event.page;
	pageSize.value = event.rows;
	void loadPortalUsers();
}

function formatDate(dateStr: string | undefined | null) {
	if (!dateStr) return "—";
	return new Date(dateStr).toLocaleString();
}

const STATE_LABEL: Record<PortalUserState, string> = {
	INVITED: "Invited",
	INVITE_EXPIRED: "Invite expired",
	ACTIVE: "Active",
	SUSPENDED: "Suspended",
};

const STATE_SEVERITY: Record<PortalUserState, string> = {
	INVITED: "info",
	INVITE_EXPIRED: "warn",
	ACTIVE: "success",
	SUSPENDED: "danger",
};

function stateLabel(state: string) {
	return STATE_LABEL[state as PortalUserState] ?? state;
}

function stateSeverity(state: string) {
	return STATE_SEVERITY[state as PortalUserState] ?? "secondary";
}

// Hover detail for the state tag: when the invite went out / lapses.
function stateTitle(user: PortalUser) {
	if (user.state === "INVITED" && user.inviteExpiresAt) {
		return `Invite expires ${formatDate(user.inviteExpiresAt)}`;
	}
	if (user.state === "INVITE_EXPIRED" && user.inviteExpiresAt) {
		return `Invite expired ${formatDate(user.inviteExpiresAt)} — the portal can re-send it`;
	}
	if (user.state === "INVITED" && user.invitedAt) {
		return `Invited ${formatDate(user.invitedAt)}`;
	}
	return "";
}

// ── Row actions ──────────────────────────────────────────────────────────

async function toggleStatus(user: PortalUser) {
	const suspend = user.status === "ACTIVE";
	try {
		if (suspend) {
			await portalUsersApi.deactivate(user.identityId, selectedClientId.value);
			toast.success("Success", `${user.email} suspended`);
		} else {
			await portalUsersApi.activate(user.identityId, selectedClientId.value);
			toast.success("Success", `${user.email} reactivated`);
		}
		await loadPortalUsers();
	} catch (e: unknown) {
		toast.error("Error", getErrorMessage(e, "Failed to update status"));
	}
}

// ── Grant a portal app (covers users with no app, and adding another) ──

const grantTarget = ref<PortalUser | null>(null);
const grantAppCode = ref("");
const granting = ref(false);
const showGrantDialog = computed({
	get: () => grantTarget.value !== null,
	set: (open: boolean) => {
		if (!open) grantTarget.value = null;
	},
});

// Active apps the user doesn't hold yet.
function grantableApps(user: PortalUser) {
	const held = new Set(user.apps.map((a) => a.code));
	return apps.value.filter((a) => a.active && !held.has(a.code));
}

function openGrant(user: PortalUser) {
	grantTarget.value = user;
	grantAppCode.value = grantableApps(user)[0]?.code ?? "";
}

async function grantApp() {
	const user = grantTarget.value;
	if (!user || !grantAppCode.value || granting.value) return;
	granting.value = true;
	try {
		await portalUsersApi.grantApp(user.identityId, selectedClientId.value, grantAppCode.value);
		const app = apps.value.find((a) => a.code === grantAppCode.value);
		toast.success("Success", `${user.email} can now sign in to ${app?.name ?? grantAppCode.value}`);
		grantTarget.value = null;
		await loadPortalUsers();
	} catch (e: unknown) {
		toast.error("Error", getErrorMessage(e, "Failed to grant portal access"));
	} finally {
		granting.value = false;
	}
}

function confirmRevoke(user: PortalUser, app: PortalUserApp) {
	confirm.require({
		message: `Remove ${user.email}'s access to "${app.name}"? Their access to this client's other portals is unaffected.`,
		header: "Remove portal access",
		icon: "pi pi-exclamation-triangle",
		acceptClass: "p-button-danger",
		acceptLabel: "Remove",
		accept: async () => {
			try {
				await portalUsersApi.revokeApp(user.identityId, selectedClientId.value, app.code);
				toast.success("Success", `${user.email} no longer has access to ${app.name}`);
				await loadPortalUsers();
			} catch (e: unknown) {
				toast.error("Error", getErrorMessage(e, "Failed to remove access"));
			}
		},
	});
}

function confirmDelete(user: PortalUser) {
	confirm.require({
		message: `Delete portal user "${user.email}"? They will no longer be able to sign in to ANY of this client's portals. This cannot be undone.`,
		header: "Delete portal user",
		icon: "pi pi-exclamation-triangle",
		acceptClass: "p-button-danger",
		acceptLabel: "Delete",
		accept: async () => {
			try {
				await portalUsersApi.remove(user.identityId, selectedClientId.value);
				toast.success("Success", `${user.email} deleted`);
				await loadPortalUsers();
			} catch (e: unknown) {
				toast.error("Error", getErrorMessage(e, "Failed to delete portal user"));
			}
		},
	});
}
</script>

<template>
  <div class="page-container">
    <div class="page-header">
      <div>
        <h1>Portal Users</h1>
        <p class="page-subtitle">
          The end users of a client's portals — separate identities from
          platform users. Portals invite their own users; manage access here.
        </p>
      </div>
    </div>

    <div class="toolbar">
      <Select
        v-model="selectedClientId"
        :options="clientOptions"
        optionLabel="label"
        optionValue="value"
        placeholder="Select a client"
        filter
        class="client-select"
      />
      <Select
        v-model="selectedAppCode"
        :options="appOptions"
        optionLabel="label"
        optionValue="value"
        placeholder="All portal apps"
        showClear
        :disabled="!selectedClientId || apps.length === 0"
        class="app-select"
      />
      <IconField class="search-field">
        <InputIcon class="pi pi-search" />
        <InputText
          v-model="search"
          placeholder="Search email or name (starts with)"
          :disabled="!selectedClientId"
          class="w-full"
        />
      </IconField>
    </div>

    <DataTable
      :value="portalUsers"
      :loading="loading"
      dataKey="identityId"
      lazy
      paginator
      :first="page * pageSize"
      :rows="pageSize"
      :totalRecords="total"
      :rowsPerPageOptions="[25, 50, 100]"
      @page="onPage"
    >
      <template #empty>
        <span v-if="!selectedClientId">Select a client to view its portal users.</span>
        <span v-else-if="search || selectedAppCode">No portal users match.</span>
        <span v-else>No portal users for this client yet.</span>
      </template>
      <Column field="email" header="Email" />
      <Column field="name" header="Name">
        <template #body="{ data }">{{ data.name || "—" }}</template>
      </Column>
      <Column field="state" header="Status">
        <template #body="{ data }">
          <Tag
            :value="stateLabel(data.state)"
            :severity="stateSeverity(data.state)"
            :title="stateTitle(data)"
          />
        </template>
      </Column>
      <Column header="Portal Apps">
        <template #body="{ data }">
          <div v-if="data.apps.length > 0" class="app-chips">
            <Chip
              v-for="app in data.apps"
              :key="app.id"
              :label="app.name"
              :title="`${app.code} · granted ${formatDate(app.grantedAt)} (${app.source})`"
              removable
              @remove="confirmRevoke(data, app)"
            />
          </div>
          <Tag
            v-else
            value="No portal app"
            severity="warn"
            title="Can't sign in through any app-linked portal — grant a portal app"
          />
        </template>
      </Column>
      <Column field="source" header="Source">
        <template #body="{ data }">
          <span class="text-muted">{{ data.source === "JIT" ? "SSO sign-in" : "Invite" }}</span>
        </template>
      </Column>
      <Column field="lastLoginAt" header="Last Login">
        <template #body="{ data }">{{ formatDate(data.lastLoginAt) }}</template>
      </Column>
      <Column field="createdAt" header="Created">
        <template #body="{ data }">{{ formatDate(data.createdAt) }}</template>
      </Column>
      <Column header="" :style="{ width: '10rem' }">
        <template #body="{ data }">
          <div class="row-actions">
            <Button
              icon="pi pi-plus-circle"
              title="Grant portal app"
              text
              rounded
              :disabled="grantableApps(data).length === 0"
              @click="openGrant(data)"
            />
            <Button
              :icon="data.status === 'ACTIVE' ? 'pi pi-ban' : 'pi pi-check-circle'"
              :title="data.status === 'ACTIVE' ? 'Suspend' : 'Reactivate'"
              text
              rounded
              @click="toggleStatus(data)"
            />
            <Button
              icon="pi pi-trash"
              title="Delete"
              text
              rounded
              severity="danger"
              @click="confirmDelete(data)"
            />
          </div>
        </template>
      </Column>
    </DataTable>

    <Dialog
      v-model:visible="showGrantDialog"
      header="Grant portal app"
      modal
      :style="{ width: '28rem' }"
    >
      <template v-if="grantTarget">
        <p class="grant-intro">
          Let <strong>{{ grantTarget.email }}</strong> sign in to another of this client's portals.
          Their password and other portals are unchanged.
        </p>
        <Select
          v-model="grantAppCode"
          :options="grantableApps(grantTarget)"
          optionLabel="name"
          optionValue="code"
          placeholder="Select a portal app"
          class="w-full"
        >
          <template #option="{ option }">{{ option.name }} <code>({{ option.code }})</code></template>
        </Select>
      </template>
      <template #footer>
        <Button label="Cancel" text :disabled="granting" @click="showGrantDialog = false" />
        <Button label="Grant" :loading="granting" :disabled="!grantAppCode" @click="grantApp" />
      </template>
    </Dialog>
  </div>
</template>

<style scoped>
.page-container {
	padding: 1.5rem;
}
.page-header {
	display: flex;
	justify-content: space-between;
	align-items: flex-start;
	margin-bottom: 1rem;
	gap: 1rem;
}
.page-header h1 {
	margin: 0 0 0.25rem;
	font-size: 1.4rem;
}
.page-subtitle {
	margin: 0;
	color: var(--p-text-muted-color);
	font-size: 0.9rem;
}
.toolbar {
	display: flex;
	flex-wrap: wrap;
	gap: 0.75rem;
	margin-bottom: 1rem;
}
.client-select {
	min-width: 16rem;
}
.app-select {
	min-width: 14rem;
}
.search-field {
	flex: 1 1 18rem;
}
.app-chips {
	display: flex;
	flex-wrap: wrap;
	gap: 0.25rem;
}
.grant-intro {
	margin-top: 0;
}
.row-actions {
	display: flex;
	gap: 0.25rem;
}
.text-muted {
	color: var(--p-text-muted-color);
}
</style>
