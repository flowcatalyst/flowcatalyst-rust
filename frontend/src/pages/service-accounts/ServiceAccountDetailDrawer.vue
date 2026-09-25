<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, watch } from "vue";
import { useRoute } from "vue-router";
import { useConfirm } from "primevue/useconfirm";
import {
	serviceAccountsApi,
	type ServiceAccount,
	type RoleAssignment,
	type RolesAssignedResponse,
	type ApplicationAccessGrant,
	type ApplicationAccessAssignedResponse,
	type AvailableApplication,
} from "@/api/service-accounts";
import { connectionsApi, type Connection } from "@/api/connections";
import type { PrincipalScope } from "@/api/users";
import { rolesApi, type Role } from "@/api/roles";
import { clientsApi, type Client } from "@/api/clients";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";
import { useDirtyForm } from "@/composables/useDirtyForm";

const emit = defineEmits<{
	changed: [];
}>();

const route = useRoute();
const confirm = useConfirm();

// Edit mode — doubles as the drawer's dirty flag.
const editMode = ref(false);

const serviceAccount = ref<ServiceAccount | null>(null);
const clients = ref<Client[]>([]);
const roleAssignments = ref<RoleAssignment[]>([]);
const availableRoles = ref<Role[]>([]);
const loading = ref(true);
const loadError = ref<string | null>(null);
const saving = ref(false);

// Edit form
const editName = ref("");
const editDescription = ref("");
const editScope = ref<PrincipalScope>("ANCHOR");
const editClientIds = ref<string[]>([]);

const { dirty, markClean, reset: resetDirty } = useDirtyForm(() => ({
	name: editName.value,
	description: editDescription.value,
	scope: editScope.value,
	clientIds: editClientIds.value,
}));

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { id, goToList } = useDrawerRoute({
	listPath: "/identity/service-accounts",
	dirty: computed(() => editMode.value && dirty.value),
});

const scopeOptions = [
	{ label: "Anchor (all clients)", value: "ANCHOR" },
	{ label: "Partner (assigned clients)", value: "PARTNER" },
	{ label: "Client (single client)", value: "CLIENT" },
];

const clientOptions = computed(() => {
	return clients.value.map((c) => ({
		label: c.name,
		value: c.id,
	}));
});

// Credentials dialogs
const showRegenerateTokenDialog = ref(false);
const showRegenerateSecretDialog = ref(false);
const newToken = ref<string | null>(null);
const newSecret = ref<string | null>(null);

// Admin-minted bearer token (POST /{id}/token) — live credential, shown once.
const showMintTokenDialog = ref(false);
const mintedToken = ref<string | null>(null);
const mintingToken = ref(false);

// Role picker dialog
const showRolePickerDialog = ref(false);
const roleSearchQuery = ref("");
const selectedRoleNames = ref<Set<string>>(new Set());
const savingRoles = ref(false);

// Application access management. Roles + app access live on the linked SERVICE
// principal, so these target /principals/{principalId}/... — principalId comes
// from the single-account read.
const principalId = computed(() => serviceAccount.value?.principalId ?? null);
const applicationAccessGrants = ref<ApplicationAccessGrant[]>([]);
const availableApplications = ref<AvailableApplication[]>([]);
const showAppPickerDialog = ref(false);
const appSearchQuery = ref("");
const selectedAppIds = ref<Set<string>>(new Set());
const savingApps = ref(false);
// Whether the account can access every application (present and future) — the
// application-axis analogue of the anchor client tier. When true the explicit
// grant list is moot and hidden. Only an all-applications admin may turn it on
// (backend-enforced); turning it off is open to any user admin.
const allApplications = ref(false);

const filteredAvailableApps = computed(() => {
	const query = appSearchQuery.value.toLowerCase();
	return availableApplications.value.filter(
		(a) =>
			a.name.toLowerCase().includes(query) ||
			a.code.toLowerCase().includes(query),
	);
});

const hasAppChanges = computed(() => {
	const currentApps = new Set(
		applicationAccessGrants.value.map((a) => a.applicationId),
	);
	if (currentApps.size !== selectedAppIds.value.size) return true;
	for (const appId of currentApps) {
		if (!selectedAppIds.value.has(appId)) return true;
	}
	return false;
});

// Connections
const connections = ref<Connection[]>([]);
const loadingConnections = ref(false);
const showCreateConnectionDialog = ref(false);

// Delete dialog
const showDeleteDialog = ref(false);
const deleting = ref(false);

const filteredAvailableRoles = computed(() => {
	const query = roleSearchQuery.value.toLowerCase();
	return availableRoles.value.filter(
		(r) =>
			r.name.toLowerCase().includes(query) ||
			r.displayName?.toLowerCase().includes(query),
	);
});

const hasRoleChanges = computed(() => {
	const currentRoles = new Set(roleAssignments.value.map((r) => r.roleName));
	if (currentRoles.size !== selectedRoleNames.value.size) return true;
	for (const role of currentRoles) {
		if (!selectedRoleNames.value.has(role)) return true;
	}
	return false;
});

// Static lookups (clients, roles) are row-independent — load them once and
// keep them across row switches.
let staticLoaded = false;

// Reactive param: the drawer instance is reused when switching between rows,
// so all per-account state resets and reloads whenever the id changes.
watch(
	id,
	async (value) => {
		if (!value) return;
		resetState();
		const staticLoads: Promise<void>[] = [];
		if (!staticLoaded) {
			staticLoads.push(loadClients(), loadAvailableRoles());
			staticLoaded = true;
		}
		await Promise.all([loadServiceAccount(), ...staticLoads]);
		if (serviceAccount.value) {
			await Promise.all([
				loadRoleAssignments(),
				loadConnections(),
				loadApplicationAccess(),
			]);
			if (route.query['edit'] === "true") {
				startEdit();
			}
		}
		loading.value = false;
	},
	{ immediate: true },
);

function resetState() {
	serviceAccount.value = null;
	loading.value = true;
	loadError.value = null;
	editMode.value = false;
	resetDirty();
	showRegenerateTokenDialog.value = false;
	showRegenerateSecretDialog.value = false;
	showMintTokenDialog.value = false;
	showRolePickerDialog.value = false;
	showAppPickerDialog.value = false;
	showDeleteDialog.value = false;
	showCreateConnectionDialog.value = false;
	newToken.value = null;
	newSecret.value = null;
	mintedToken.value = null;
	roleAssignments.value = [];
	applicationAccessGrants.value = [];
	availableApplications.value = [];
	allApplications.value = false;
	connections.value = [];
}

async function loadServiceAccount() {
	const said = id.value;
	if (!said) return;
	try {
		serviceAccount.value = await serviceAccountsApi.get(said);
		editName.value = serviceAccount.value.name;
		editDescription.value = serviceAccount.value.description || "";
		// The wire only ever carries ANCHOR/PARTNER/CLIENT; the generated type
		// is plain string (spec has no enums).
		editScope.value = (serviceAccount.value.scope as PrincipalScope) || "ANCHOR";
		editClientIds.value = serviceAccount.value.clientIds || [];
	} catch (error) {
		console.error("Failed to fetch service account:", error);
		loadError.value = "Service account not found";
	}
}

async function loadClients() {
	try {
		const response = await clientsApi.list();
		clients.value = response.clients;
	} catch (error) {
		console.error("Failed to fetch clients:", error);
	}
}

async function loadAvailableRoles() {
	try {
		const response = await rolesApi.list();
		availableRoles.value = response.items;
	} catch (error) {
		console.error("Failed to fetch available roles:", error);
	}
}

async function loadRoleAssignments() {
	const said = id.value;
	if (!said) return;
	try {
		const response = await serviceAccountsApi.getRoles(said);
		roleAssignments.value = response.roles;
	} catch (error) {
		console.error("Failed to fetch role assignments:", error);
	}
}

async function loadApplicationAccess() {
	if (!principalId.value) return;
	try {
		const response = await serviceAccountsApi.getApplicationAccess(
			principalId.value,
		);
		applicationAccessGrants.value = response.applications;
		allApplications.value = response.allApplications;
	} catch (error) {
		console.error("Failed to fetch application access:", error);
	}
}

async function loadAvailableApplications() {
	if (!principalId.value) return;
	try {
		const response = await serviceAccountsApi.getAvailableApplications(
			principalId.value,
		);
		availableApplications.value = response.applications;
	} catch (error) {
		console.error("Failed to fetch available applications:", error);
	}
}

// Toggle the "access to all applications" flag. Preserves the current explicit
// grant list so flipping back off restores the prior selection. One-way: on
// failure the switch reverts.
async function onToggleAllApplications(value: boolean) {
	if (!principalId.value) return;
	savingApps.value = true;
	try {
		const applicationIds = applicationAccessGrants.value.map(
			(a) => a.applicationId,
		);
		const response = await serviceAccountsApi.assignApplicationAccess(
			principalId.value,
			applicationIds,
			value,
		);
		allApplications.value = response.allApplications;
		applicationAccessGrants.value = response.applications;
		toast.success(
			"Success",
			value
				? "Granted access to all applications"
				: "Restricted to specific applications",
		);
		emit("changed");
	} catch (e: unknown) {
		allApplications.value = !value;
	} finally {
		savingApps.value = false;
	}
}

async function openAppPicker() {
	if (availableApplications.value.length === 0) {
		await loadAvailableApplications();
	}
	selectedAppIds.value = new Set(
		applicationAccessGrants.value.map((a) => a.applicationId),
	);
	appSearchQuery.value = "";
	showAppPickerDialog.value = true;
}

function toggleApp(appId: string) {
	if (selectedAppIds.value.has(appId)) {
		selectedAppIds.value.delete(appId);
	} else {
		selectedAppIds.value.add(appId);
	}
	selectedAppIds.value = new Set(selectedAppIds.value);
}

function removeSelectedApp(appId: string) {
	selectedAppIds.value.delete(appId);
	selectedAppIds.value = new Set(selectedAppIds.value);
}

function cancelAppPicker() {
	showAppPickerDialog.value = false;
}

async function saveApps() {
	if (!principalId.value) return;
	savingApps.value = true;
	try {
		const applicationIds = Array.from(selectedAppIds.value);
		const response: ApplicationAccessAssignedResponse =
			await serviceAccountsApi.assignApplicationAccess(
				principalId.value,
				applicationIds,
			);
		applicationAccessGrants.value = response.applications;
		allApplications.value = response.allApplications;
		showAppPickerDialog.value = false;

		const added = response.added;
		const removed = response.removed;
		let detail = "Application access updated";
		if (added > 0 && removed > 0) {
			detail = `Added ${added} app(s), removed ${removed} app(s)`;
		} else if (added > 0) {
			detail = `Added ${added} app(s)`;
		} else if (removed > 0) {
			detail = `Removed ${removed} app(s)`;
		}

		toast.success("Success", detail);
		emit("changed");
	} catch (e: unknown) {
	} finally {
		savingApps.value = false;
	}
}

function getAppDisplay(appId: string) {
	const app = availableApplications.value.find((a) => a.id === appId);
	return {
		name: app?.name || appId,
		code: app?.code || "",
	};
}

async function loadConnections() {
	const said = id.value;
	if (!said) return;
	loadingConnections.value = true;
	try {
		const clientScope = serviceAccount.value?.clientIds?.[0];
		const response = await connectionsApi.list(
			clientScope ? { clientId: clientScope } : {},
		);
		connections.value = response.connections.filter(
			(c) => c.serviceAccountId === said,
		);
	} catch (error) {
		console.error("Failed to fetch connections:", error);
	} finally {
		loadingConnections.value = false;
	}
}

function onConnectionCreated(_connection: Connection) {
	loadConnections();
}

function confirmPauseConnection(connection: Connection) {
	confirm.require({
		message: `Are you sure you want to pause connection "${connection.name}"?`,
		header: "Pause Connection",
		icon: "pi pi-pause-circle",
		acceptClass: "p-button-warning",
		accept: () => pauseConnection(connection.id),
	});
}

async function pauseConnection(connectionId: string) {
	try {
		await connectionsApi.pause(connectionId);
		toast.success("Success", "Connection paused");
		await loadConnections();
	} catch (e: unknown) {
	}
}

async function activateConnection(connectionId: string) {
	try {
		await connectionsApi.activate(connectionId);
		toast.success("Success", "Connection activated");
		await loadConnections();
	} catch (e: unknown) {
	}
}

function startEdit() {
	editName.value = serviceAccount.value?.name || "";
	editDescription.value = serviceAccount.value?.description || "";
	// The wire only ever carries ANCHOR/PARTNER/CLIENT; the generated type
	// is plain string (spec has no enums).
	editScope.value = (serviceAccount.value?.scope as PrincipalScope) || "ANCHOR";
	editClientIds.value = serviceAccount.value?.clientIds || [];
	editMode.value = true;
	markClean();
}

function cancelEdit() {
	editName.value = serviceAccount.value?.name || "";
	editDescription.value = serviceAccount.value?.description || "";
	// The wire only ever carries ANCHOR/PARTNER/CLIENT; the generated type
	// is plain string (spec has no enums).
	editScope.value = (serviceAccount.value?.scope as PrincipalScope) || "ANCHOR";
	editClientIds.value = serviceAccount.value?.clientIds || [];
	editMode.value = false;
	resetDirty();
}

async function saveServiceAccount() {
	const said = id.value;
	if (!said) return;
	if (!editName.value.trim()) {
		toast.error("Error", "Name is required");
		return;
	}

	saving.value = true;
	try {
		await serviceAccountsApi.update(said, {
			name: editName.value,
			description: editDescription.value || undefined,
			scope: editScope.value,
			clientIds: editClientIds.value,
		});
		serviceAccount.value!.name = editName.value;
		serviceAccount.value!.description = editDescription.value;
		serviceAccount.value!.scope = editScope.value;
		serviceAccount.value!.clientIds = editClientIds.value;
		editMode.value = false;
		resetDirty();
		toast.success("Success", "Service account updated successfully");
		emit("changed");
	} catch (e: unknown) {
	} finally {
		saving.value = false;
	}
}

async function mintBearerToken() {
	const said = id.value;
	if (!said) return;
	mintingToken.value = true;
	try {
		const response = await serviceAccountsApi.mintToken(said);
		mintedToken.value = response.accessToken;
		showMintTokenDialog.value = true;
	} catch (e: unknown) {
	} finally {
		mintingToken.value = false;
	}
}

async function regenerateToken() {
	const said = id.value;
	if (!said) return;
	saving.value = true;
	try {
		const response = await serviceAccountsApi.regenerateToken(said);
		newToken.value = response.authToken ?? null;
		showRegenerateTokenDialog.value = true;
		toast.success("Success", "Auth token regenerated");
	} catch (e: unknown) {
	} finally {
		saving.value = false;
	}
}

async function regenerateSecret() {
	const said = id.value;
	if (!said) return;
	saving.value = true;
	try {
		const response = await serviceAccountsApi.regenerateSecret(said);
		newSecret.value = response.signingSecret ?? null;
		showRegenerateSecretDialog.value = true;
		toast.success("Success", "Signing secret regenerated");
	} catch (e: unknown) {
	} finally {
		saving.value = false;
	}
}

function copyToClipboard(text: string, label: string) {
	navigator.clipboard.writeText(text);
	toast.info("Copied", `${label} copied to clipboard`);
}

function openRolePicker() {
	selectedRoleNames.value = new Set(
		roleAssignments.value.map((r) => r.roleName),
	);
	roleSearchQuery.value = "";
	showRolePickerDialog.value = true;
}

function toggleRole(roleName: string) {
	if (selectedRoleNames.value.has(roleName)) {
		selectedRoleNames.value.delete(roleName);
	} else {
		selectedRoleNames.value.add(roleName);
	}
	selectedRoleNames.value = new Set(selectedRoleNames.value);
}

function removeSelectedRole(roleName: string) {
	selectedRoleNames.value.delete(roleName);
	selectedRoleNames.value = new Set(selectedRoleNames.value);
}

async function saveRoles() {
	const said = id.value;
	if (!said) return;
	savingRoles.value = true;
	try {
		const roles = Array.from(selectedRoleNames.value);
		const response: RolesAssignedResponse =
			await serviceAccountsApi.assignRoles(said, roles);
		roleAssignments.value = response.roles;
		if (serviceAccount.value) {
			serviceAccount.value.roles = roles;
		}
		showRolePickerDialog.value = false;

		const added = response.addedRoles.length;
		const removed = response.removedRoles.length;
		let detail = "Roles updated";
		if (added > 0 && removed > 0) {
			detail = `Added ${added} role(s), removed ${removed} role(s)`;
		} else if (added > 0) {
			detail = `Added ${added} role(s)`;
		} else if (removed > 0) {
			detail = `Removed ${removed} role(s)`;
		}

		toast.success("Success", detail);
		emit("changed");
	} catch (e: unknown) {
	} finally {
		savingRoles.value = false;
	}
}

function getRoleDisplay(roleName: string) {
	const role = availableRoles.value.find((r) => r.name === roleName);
	return {
		displayName: role?.displayName || roleName.split(":").pop() || roleName,
		fullName: roleName,
	};
}

function getClientName(clientId: string): string {
	const client = clients.value.find((c) => c.id === clientId);
	return client?.name || clientId;
}

function getClientNames(clientIds: string[]): string {
	if (!clientIds || clientIds.length === 0)
		return "All clients (no restriction)";
	return clientIds.map((cid) => getClientName(cid)).join(", ");
}

function formatDate(dateStr: string | null | undefined) {
	if (!dateStr) return "—";
	return new Date(dateStr).toLocaleDateString();
}

async function deleteServiceAccount() {
	const said = id.value;
	if (!said) return;
	deleting.value = true;
	try {
		await serviceAccountsApi.delete(said);
		toast.success("Success", "Service account deleted successfully");
		emit("changed");
		editMode.value = false;
		void drawer.value?.close(true);
	} catch (e: unknown) {
	} finally {
		deleting.value = false;
		showDeleteDialog.value = false;
	}
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    :title="serviceAccount?.name || 'Service Account'"
    :subtitle="serviceAccount?.code"
    size="wide"
    :loading="loading"
    :error="loadError"
    :dirty="editMode && dirty"
    @close="goToList()"
  >
    <template v-if="serviceAccount" #header-extra>
      <Tag
        :value="serviceAccount.active ? 'Active' : 'Inactive'"
        :severity="serviceAccount.active ? 'success' : 'danger'"
      />
    </template>

    <template v-if="serviceAccount">
      <!-- Service Account Information -->
      <FcFormSection title="Service Account Information" flat>
        <template #actions>
          <Button v-if="!editMode" label="Edit" icon="pi pi-pencil" text @click="startEdit" />
          <template v-else>
            <Button v-if="dirty" label="Discard" text @click="cancelEdit" />
            <Button
              label="Save"
              icon="pi pi-check"
              :disabled="!dirty"
              :loading="saving"
              @click="saveServiceAccount"
            />
          </template>
        </template>

        <!-- View mode -->
        <div v-if="!editMode" class="fc-detail-grid">
          <FcDetailField label="Name" :value="serviceAccount.name" />
          <FcDetailField label="Code">
            <code>{{ serviceAccount.code }}</code>
          </FcDetailField>
          <FcDetailField v-if="serviceAccount.principalId" label="Principal ID">
            <code>{{ serviceAccount.principalId }}</code>
          </FcDetailField>
          <FcDetailField v-if="serviceAccount.oauthClientId" label="OAuth Client ID">
            <code>{{ serviceAccount.oauthClientId }}</code>
          </FcDetailField>
          <FcDetailField label="Description" :value="serviceAccount.description" span />
          <FcDetailField label="Scope">
            <Tag
              :value="serviceAccount.scope || 'N/A'"
              :severity="
                serviceAccount.scope === 'ANCHOR'
                  ? 'success'
                  : serviceAccount.scope === 'PARTNER'
                    ? 'info'
                    : 'warn'
              "
            />
          </FcDetailField>
          <FcDetailField
            v-if="serviceAccount.scope !== 'ANCHOR'"
            label="Client Access"
            :value="getClientNames(serviceAccount.clientIds)"
            span
          />
          <FcDetailField label="Auth Type">
            <Tag :value="serviceAccount.authType || 'BEARER'" severity="secondary" />
          </FcDetailField>
          <FcDetailField label="Created" :value="formatDate(serviceAccount.createdAt)" />
          <FcDetailField label="Last Used" :value="formatDate(serviceAccount.lastUsedAt)" />
        </div>

        <!-- Edit mode -->
        <div v-else class="fc-form-grid">
          <FcFormField label="Name" required>
            <template #default="{ id: fieldId }">
              <InputText :id="fieldId" v-model="editName" />
            </template>
          </FcFormField>
          <FcFormField label="Scope">
            <template #default="{ id: fieldId }">
              <Select
                :id="fieldId"
                v-model="editScope"
                :options="scopeOptions"
                optionLabel="label"
                optionValue="value"
              />
            </template>
          </FcFormField>
          <FcFormField label="Description" span>
            <template #default="{ id: fieldId }">
              <Textarea :id="fieldId" v-model="editDescription" rows="2" />
            </template>
          </FcFormField>
          <FcFormField v-if="editScope !== 'ANCHOR'" label="Client Access" span>
            <template #default="{ id: fieldId }">
              <MultiSelect
                :id="fieldId"
                v-model="editClientIds"
                :options="clientOptions"
                optionLabel="label"
                optionValue="value"
                placeholder="Select clients..."
                display="chip"
                filter
              />
            </template>
          </FcFormField>
        </div>
      </FcFormSection>

      <!-- Webhook Credentials -->
      <FcFormSection title="Webhook Credentials" flat>
        <div class="credentials-section">
          <p class="credentials-info">
            Credentials are encrypted and cannot be viewed. You can regenerate them if needed.
          </p>

          <div class="credentials-actions">
            <div class="credential-action">
              <span class="credential-label">Auth Token (Bearer)</span>
              <Button
                label="Regenerate Token"
                icon="pi pi-refresh"
                outlined
                :loading="saving"
                @click="regenerateToken"
              />
            </div>

            <div class="credential-action">
              <span class="credential-label">Signing Secret (HMAC-SHA256)</span>
              <Button
                label="Regenerate Secret"
                icon="pi pi-refresh"
                outlined
                :loading="saving"
                @click="regenerateSecret"
              />
            </div>
          </div>
        </div>
      </FcFormSection>

      <!-- API Access -->
      <FcFormSection title="API Access" flat>
        <div class="credentials-section">
          <p class="credentials-info">
            Mint a bearer token with this account's permissions — the same
            token a client-credentials exchange returns, without needing the
            client secret. Expires after 1 hour; the mint is audited.
          </p>
          <div class="credentials-actions">
            <div class="credential-action">
              <span class="credential-label">Bearer Token (1 hour)</span>
              <Button
                label="Generate Token"
                icon="pi pi-key"
                outlined
                :loading="mintingToken"
                :disabled="!serviceAccount?.active"
                @click="mintBearerToken"
              />
            </div>
          </div>
        </div>
      </FcFormSection>

      <!-- Roles -->
      <FcFormSection title="Roles" flat>
        <template #actions>
          <Button label="Manage Roles" icon="pi pi-pencil" text @click="openRolePicker" />
        </template>

        <div v-if="roleAssignments.length === 0" class="no-roles-notice">
          <p>No roles assigned to this service account.</p>
          <Button label="Assign Roles" icon="pi pi-plus" text @click="openRolePicker" />
        </div>

        <DataTable v-else :value="roleAssignments" size="small">
          <Column field="roleName" header="Role">
            <template #body="{ data }">
              <div class="role-cell">
                <span class="role-name">{{ data.roleName.split(':').pop() }}</span>
                <span class="role-full-name">{{ data.roleName }}</span>
              </div>
            </template>
          </Column>
          <Column field="assignmentSource" header="Source">
            <template #body="{ data }">
              <Tag
                :value="data.assignmentSource"
                :severity="data.assignmentSource === 'MANUAL' ? 'info' : 'secondary'"
              />
            </template>
          </Column>
          <Column field="assignedAt" header="Assigned">
            <template #body="{ data }">
              {{ formatDate(data.assignedAt) }}
            </template>
          </Column>
        </DataTable>
      </FcFormSection>

      <!-- Application Access -->
      <FcFormSection title="Application Access" flat>
        <template v-if="!allApplications" #actions>
          <Button
            label="Manage Applications"
            icon="pi pi-pencil"
            text
            @click="openAppPicker"
          />
        </template>

        <!-- All-applications toggle: the application-axis analogue of an anchor. -->
        <div class="all-apps-toggle">
          <ToggleSwitch
            inputId="allApplications"
            v-model="allApplications"
            :disabled="savingApps"
            @update:modelValue="onToggleAllApplications"
          />
          <label for="allApplications" class="all-apps-label">
            <span class="all-apps-title">Access to all applications</span>
            <span class="all-apps-hint">
              This service account can access every application, present and future.
            </span>
          </label>
        </div>

        <template v-if="!allApplications">
          <div v-if="applicationAccessGrants.length === 0" class="no-apps-notice">
            <p>No application access granted to this service account.</p>
            <Button label="Grant Application Access" icon="pi pi-plus" text @click="openAppPicker" />
          </div>

          <DataTable v-else :value="applicationAccessGrants" size="small">
            <Column field="applicationName" header="Application">
              <template #body="{ data }">
                <div class="app-cell">
                  <span class="app-name">{{ data.applicationName || data.applicationId }}</span>
                  <span class="app-code">{{ data.applicationCode }}</span>
                </div>
              </template>
            </Column>
          </DataTable>
        </template>
      </FcFormSection>

      <!-- Connections -->
      <FcFormSection title="Connections" flat>
        <template #actions>
          <Button
            label="New Connection"
            icon="pi pi-plus"
            text
            @click="showCreateConnectionDialog = true"
          />
        </template>

        <ProgressSpinner v-if="loadingConnections" strokeWidth="3" style="width: 32px; height: 32px" />

        <div v-else-if="connections.length === 0" class="no-connections-notice">
          <p>No connections for this service account.</p>
          <Button label="Create Connection" icon="pi pi-plus" text @click="showCreateConnectionDialog = true" />
        </div>

        <DataTable v-else :value="connections" size="small">
          <Column field="code" header="Code">
            <template #body="{ data }">
              <router-link :to="`/connections/${data.id}`" class="code-link">
                {{ data.code }}
              </router-link>
            </template>
          </Column>
          <Column field="endpoint" header="Endpoint">
            <template #body="{ data }">
              <span class="endpoint-text" :title="data.endpoint">{{ data.endpoint }}</span>
            </template>
          </Column>
          <Column field="status" header="Status">
            <template #body="{ data }">
              <Tag
                :value="data.status"
                :severity="data.status === 'ACTIVE' ? 'success' : 'warn'"
              />
            </template>
          </Column>
          <Column header="Actions" style="width: 80px">
            <template #body="{ data }">
              <Button
                v-if="data.status === 'ACTIVE'"
                icon="pi pi-pause"
                text
                rounded
                severity="warn"
                size="small"
                v-tooltip.top="'Pause'"
                @click="confirmPauseConnection(data)"
              />
              <Button
                v-else
                icon="pi pi-play"
                text
                rounded
                severity="success"
                size="small"
                v-tooltip.top="'Activate'"
                @click="activateConnection(data.id)"
              />
            </template>
          </Column>
        </DataTable>
      </FcFormSection>

      <!-- Danger Zone -->
      <FcFormSection title="Danger Zone" flat>
        <div class="action-items">
          <div class="action-item">
            <div class="action-info">
              <strong>Delete Service Account</strong>
              <p>Permanently delete this service account and its credentials. Cannot be undone.</p>
            </div>
            <Button
              label="Delete"
              icon="pi pi-trash"
              severity="danger"
              outlined
              @click="showDeleteDialog = true"
            />
          </div>
        </div>
      </FcFormSection>

      <ConnectionCreateDialog
        v-model:visible="showCreateConnectionDialog"
        :service-account-id="serviceAccount.id"
        :client-id="serviceAccount?.clientIds?.[0]"
        @created="onConnectionCreated"
      />
    </template>

    <!-- Regenerate Token Dialog -->
    <Dialog
      v-model:visible="showRegenerateTokenDialog"
      header="New Auth Token"
      :style="{ width: '500px' }"
      :modal="true"
    >
      <div class="credential-dialog">
        <p class="warning-text">
          <i class="pi pi-exclamation-triangle"></i>
          Copy this token now. It will not be shown again.
        </p>
        <div class="credential-value">
          <code>{{ newToken }}</code>
          <Button
            icon="pi pi-copy"
            text
            rounded
            @click="copyToClipboard(newToken!, 'Token')"
            v-tooltip.top="'Copy'"
          />
        </div>
      </div>
      <template #footer>
        <Button label="Done" @click="showRegenerateTokenDialog = false" />
      </template>
    </Dialog>

    <!-- Regenerate Secret Dialog -->
    <Dialog
      v-model:visible="showRegenerateSecretDialog"
      header="New Signing Secret"
      :style="{ width: '500px' }"
      :modal="true"
    >
      <div class="credential-dialog">
        <p class="warning-text">
          <i class="pi pi-exclamation-triangle"></i>
          Copy this secret now. It will not be shown again.
        </p>
        <div class="credential-value">
          <code>{{ newSecret }}</code>
          <Button
            icon="pi pi-copy"
            text
            rounded
            @click="copyToClipboard(newSecret!, 'Secret')"
            v-tooltip.top="'Copy'"
          />
        </div>
      </div>
      <template #footer>
        <Button label="Done" @click="showRegenerateSecretDialog = false" />
      </template>
    </Dialog>

    <!-- Minted Bearer Token Dialog -->
    <Dialog
      v-model:visible="showMintTokenDialog"
      header="Bearer Token"
      :style="{ width: '560px' }"
      :modal="true"
      @hide="mintedToken = null"
    >
      <div class="credential-dialog">
        <p class="warning-text">
          <i class="pi pi-exclamation-triangle"></i>
          This token carries the service account's full permissions and
          expires in 1 hour. Copy it now — it will not be shown again.
        </p>
        <div class="credential-value">
          <code>{{ mintedToken }}</code>
          <Button
            icon="pi pi-copy"
            text
            rounded
            @click="copyToClipboard(mintedToken!, 'Token')"
            v-tooltip.top="'Copy'"
          />
        </div>
      </div>
      <template #footer>
        <Button label="Done" @click="showMintTokenDialog = false" />
      </template>
    </Dialog>

    <!-- Role Picker Dialog -->
    <Dialog
      v-model:visible="showRolePickerDialog"
      header="Manage Roles"
      :style="{ width: '700px' }"
      :modal="true"
      :closable="!savingRoles"
    >
      <div class="role-picker">
        <div class="role-pane available-roles">
          <div class="pane-header">
            <h4>Available Roles</h4>
            <InputText
              v-model="roleSearchQuery"
              placeholder="Filter roles..."
              class="role-filter"
            />
          </div>
          <div class="role-list">
            <div
              v-for="role in filteredAvailableRoles"
              :key="role.name"
              class="role-item"
              :class="{ selected: selectedRoleNames.has(role.name) }"
              @click="toggleRole(role.name)"
            >
              <div class="role-item-content">
                <span class="role-display-name">{{ role.displayName || role.name }}</span>
                <span class="role-name-code">{{ role.name }}</span>
              </div>
              <i v-if="selectedRoleNames.has(role.name)" class="pi pi-check check-icon"></i>
            </div>
            <div v-if="filteredAvailableRoles.length === 0" class="no-results">No roles found</div>
          </div>
        </div>

        <div class="role-pane selected-roles">
          <div class="pane-header">
            <h4>Selected Roles ({{ selectedRoleNames.size }})</h4>
          </div>
          <div class="role-list">
            <div
              v-for="roleName in selectedRoleNames"
              :key="roleName"
              class="role-item selected-item"
            >
              <div class="role-item-content">
                <span class="role-display-name">{{ getRoleDisplay(roleName).displayName }}</span>
                <span class="role-name-code">{{ roleName }}</span>
              </div>
              <Button
                icon="pi pi-times"
                text
                rounded
                severity="danger"
                size="small"
                @click="removeSelectedRole(roleName)"
                v-tooltip.top="'Remove'"
              />
            </div>
            <div v-if="selectedRoleNames.size === 0" class="no-results">No roles selected</div>
          </div>
        </div>
      </div>

      <template #footer>
        <Button label="Cancel" text @click="showRolePickerDialog = false" :disabled="savingRoles" />
        <Button
          label="Save Roles"
          icon="pi pi-check"
          :disabled="!hasRoleChanges"
          :loading="savingRoles"
          @click="saveRoles"
        />
      </template>
    </Dialog>

    <!-- Application Picker Dialog (Dual-Pane) -->
    <Dialog
      v-model:visible="showAppPickerDialog"
      header="Manage Application Access"
      :style="{ width: '700px' }"
      :modal="true"
      :closable="!savingApps"
    >
      <div class="app-picker">
        <!-- Left Pane: Available Applications -->
        <div class="app-pane available-apps">
          <div class="pane-header">
            <h4>Available Applications</h4>
            <InputText
              v-model="appSearchQuery"
              placeholder="Filter applications..."
              class="app-filter"
            />
          </div>
          <div class="app-list">
            <div
              v-for="app in filteredAvailableApps"
              :key="app.id"
              class="app-item"
              :class="{ selected: selectedAppIds.has(app.id) }"
              @click="toggleApp(app.id)"
            >
              <div class="app-item-content">
                <span class="app-display-name">{{ app.name }}</span>
                <span class="app-name-code">{{ app.code }}</span>
              </div>
              <i v-if="selectedAppIds.has(app.id)" class="pi pi-check check-icon"></i>
            </div>
            <div v-if="filteredAvailableApps.length === 0" class="no-results">
              No applications found
            </div>
          </div>
        </div>

        <!-- Right Pane: Selected Applications -->
        <div class="app-pane selected-apps">
          <div class="pane-header">
            <h4>Selected Applications ({{ selectedAppIds.size }})</h4>
          </div>
          <div class="app-list">
            <div v-for="appId in selectedAppIds" :key="appId" class="app-item selected-item">
              <div class="app-item-content">
                <span class="app-display-name">{{ getAppDisplay(appId).name }}</span>
                <span class="app-name-code">{{ getAppDisplay(appId).code }}</span>
              </div>
              <Button
                icon="pi pi-times"
                text
                rounded
                severity="danger"
                size="small"
                @click="removeSelectedApp(appId)"
                v-tooltip.top="'Remove'"
              />
            </div>
            <div v-if="selectedAppIds.size === 0" class="no-results">No applications selected</div>
          </div>
        </div>
      </div>

      <template #footer>
        <Button label="Cancel" text @click="cancelAppPicker" :disabled="savingApps" />
        <Button
          label="Save Application Access"
          icon="pi pi-check"
          :disabled="!hasAppChanges"
          :loading="savingApps"
          @click="saveApps"
        />
      </template>
    </Dialog>

    <!-- Delete Confirmation Dialog -->
    <Dialog
      v-model:visible="showDeleteDialog"
      header="Delete Service Account"
      :style="{ width: '450px' }"
      :modal="true"
      :closable="!deleting"
    >
      <div class="delete-dialog">
        <p class="delete-warning">
          <i class="pi pi-exclamation-triangle"></i>
          Are you sure you want to delete this service account?
        </p>
        <p class="delete-details">
          This will permanently delete <strong>{{ serviceAccount?.name }}</strong> including:
        </p>
        <ul class="delete-list">
          <li>The service account and all webhook credentials</li>
          <li>The associated principal and role assignments</li>
          <li>The OAuth client (client_credentials will stop working)</li>
        </ul>
        <p class="delete-note">This action cannot be undone.</p>
      </div>
      <template #footer>
        <Button label="Cancel" text @click="showDeleteDialog = false" :disabled="deleting" />
        <Button
          label="Delete"
          icon="pi pi-trash"
          severity="danger"
          :loading="deleting"
          @click="deleteServiceAccount"
        />
      </template>
    </Dialog>
  </EntityDrawer>
</template>

<style scoped>
.credentials-section {
  padding: 16px;
  background: #f8fafc;
  border-radius: 8px;
}

.credentials-info {
  font-size: 14px;
  color: #64748b;
  margin: 0 0 16px 0;
}

.credentials-actions {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.credential-action {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px;
  background: white;
  border: 1px solid #e2e8f0;
  border-radius: 6px;
}

.credential-label {
  font-size: 14px;
  font-weight: 500;
  color: #1e293b;
}

.credential-dialog {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.warning-text {
  display: flex;
  align-items: center;
  gap: 8px;
  color: #f59e0b;
  font-size: 14px;
  margin: 0;
}

.credential-value {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px;
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 6px;
}

.credential-value code {
  flex: 1;
  font-size: 12px;
  word-break: break-all;
}

.no-roles-notice {
  text-align: center;
  padding: 24px;
  color: #64748b;
}

.no-roles-notice p {
  margin: 0 0 12px 0;
}

.role-cell {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.role-name {
  font-size: 14px;
  font-weight: 500;
  color: #1e293b;
}

.role-full-name {
  font-size: 12px;
  color: #64748b;
  font-family: monospace;
}

.no-connections-notice {
  text-align: center;
  padding: 24px;
  color: #64748b;
}

.no-connections-notice p {
  margin: 0 0 12px 0;
}

.code-link {
  font-family: monospace;
  font-size: 13px;
  color: #3b82f6;
  text-decoration: none;
}

.code-link:hover {
  text-decoration: underline;
}

.endpoint-text {
  font-size: 13px;
  color: #64748b;
  max-width: 300px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  display: inline-block;
}

/* Danger zone action row */
.action-items {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.action-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  padding: 16px;
  background: #fafafa;
  border-radius: 8px;
  border: 1px solid #e5e7eb;
}

.action-info strong {
  display: block;
  margin-bottom: 4px;
}

.action-info p {
  margin: 0;
  font-size: 13px;
  color: #64748b;
}

/* Dual-pane role picker styles */
.role-picker {
  display: flex;
  gap: 16px;
  min-height: 350px;
}

.role-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  border: 1px solid #e2e8f0;
  border-radius: 8px;
  overflow: hidden;
}

.pane-header {
  padding: 12px;
  background: #f8fafc;
  border-bottom: 1px solid #e2e8f0;
}

.pane-header h4 {
  margin: 0 0 8px 0;
  font-size: 13px;
  font-weight: 600;
  color: #475569;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.selected-roles .pane-header h4 {
  margin-bottom: 0;
}

.role-filter {
  width: 100%;
}

.role-list {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
}

.role-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  border-radius: 6px;
  cursor: pointer;
  transition: background-color 0.15s;
}

.role-item:hover {
  background: #f1f5f9;
}

.role-item.selected {
  background: #eff6ff;
}

.role-item.selected-item {
  background: #f8fafc;
  cursor: default;
}

.role-item-content {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.role-item-content .role-display-name {
  font-size: 13px;
  font-weight: 500;
  color: #1e293b;
}

.role-item-content .role-name-code {
  font-size: 11px;
  color: #64748b;
  font-family: monospace;
}

.check-icon {
  color: #3b82f6;
  font-size: 14px;
}

.no-results {
  padding: 20px;
  text-align: center;
  color: #94a3b8;
  font-size: 13px;
}

/* Delete dialog styles */
.delete-dialog {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.delete-warning {
  display: flex;
  align-items: center;
  gap: 10px;
  color: #dc2626;
  font-size: 15px;
  font-weight: 500;
  margin: 0;
}

.delete-warning i {
  font-size: 20px;
}

.delete-details {
  font-size: 14px;
  color: #374151;
  margin: 0;
}

.delete-list {
  font-size: 13px;
  color: #6b7280;
  margin: 0;
  padding-left: 20px;
}

.delete-list li {
  margin-bottom: 4px;
}

.delete-note {
  font-size: 13px;
  color: #9ca3af;
  font-style: italic;
  margin: 0;
}

/* Application access section */
.all-apps-toggle {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 4px 0 16px 0;
}

.all-apps-label {
  display: flex;
  flex-direction: column;
  gap: 2px;
  cursor: pointer;
}

.all-apps-title {
  font-weight: 600;
  font-size: 14px;
}

.all-apps-hint {
  font-size: 12px;
  color: #64748b;
}

.no-apps-notice {
  text-align: center;
  padding: 24px;
  color: #64748b;
}

.no-apps-notice p {
  margin: 0 0 12px 0;
}

.app-cell {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.app-name {
  font-size: 14px;
  font-weight: 500;
  color: #1e293b;
}

.app-code {
  font-size: 12px;
  color: #64748b;
  font-family: monospace;
}

/* Dual-pane application picker (mirrors the role picker) */
.app-picker {
  display: flex;
  gap: 16px;
  min-height: 350px;
}

.app-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  border: 1px solid #e2e8f0;
  border-radius: 8px;
  overflow: hidden;
}

.selected-apps .pane-header h4 {
  margin-bottom: 0;
}

.app-filter {
  width: 100%;
}

.app-list {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
}

.app-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  border-radius: 6px;
  cursor: pointer;
  transition: background-color 0.15s;
}

.app-item:hover {
  background: #f1f5f9;
}

.app-item.selected {
  background: #eff6ff;
}

.app-item.selected-item {
  background: #f8fafc;
  cursor: default;
}

.app-item.selected-item:hover {
  background: #f1f5f9;
}

.app-item-content {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.app-item-content .app-display-name {
  font-size: 13px;
  font-weight: 500;
  color: #1e293b;
}

.app-item-content .app-name-code {
  font-size: 11px;
  color: #64748b;
  font-family: monospace;
}

@media (max-width: 768px) {
  .role-picker,
  .app-picker {
    flex-direction: column;
    min-height: 500px;
  }

  .role-pane,
  .app-pane {
    min-height: 200px;
  }
}
</style>
