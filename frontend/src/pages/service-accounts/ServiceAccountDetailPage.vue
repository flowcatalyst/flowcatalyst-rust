<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import { useConfirm } from "primevue/useconfirm";
import {
	serviceAccountsApi,
	type ServiceAccount,
	type RoleAssignment,
	type RolesAssignedResponse,
} from "@/api/service-accounts";
import { connectionsApi, type Connection } from "@/api/connections";
import {
	usersApi,
	type PrincipalScope,
	type ApplicationAccessGrant,
	type AvailableApplication,
} from "@/api/users";
import { rolesApi, type Role } from "@/api/roles";
import { clientsApi, type Client } from "@/api/clients";
import { useReturnTo } from "@/composables/useReturnTo";
import { assignmentSourceSeverity } from "@/utils/roleAssignment";

const route = useRoute();
const { returnTo } = useReturnTo();
const confirm = useConfirm();

const serviceAccountId = route.params['id'] as string;

const serviceAccount = ref<ServiceAccount | null>(null);
const clients = ref<Client[]>([]);
const roleAssignments = ref<RoleAssignment[]>([]);
const availableRoles = ref<Role[]>([]);
const loading = ref(true);
const saving = ref(false);

// Edit mode
const editMode = ref(false);
const editName = ref("");
const editDescription = ref("");
const editScope = ref<PrincipalScope>("ANCHOR");
const editClientIds = ref<string[]>([]);

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

// Role picker dialog
const showRolePickerDialog = ref(false);
const roleSearchQuery = ref("");
const selectedRoleNames = ref<Set<string>>(new Set());
const savingRoles = ref(false);

// Application access (on the linked principal; the service account id is
// the principal id). A new account has none until granted here.
const appAccessGrants = ref<ApplicationAccessGrant[]>([]);
const allApplications = ref(false);
const availableApplications = ref<AvailableApplication[]>([]);
const appEditMode = ref(false);
const editAppIds = ref<string[]>([]);
const editAllApplications = ref(false);
const savingApps = ref(false);

const appOptions = computed(() =>
	availableApplications.value.map((a) => ({
		label: `${a.name} (${a.code})`,
		value: a.id,
	})),
);

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

onMounted(async () => {
	await Promise.all([
		loadServiceAccount(),
		loadClients(),
		loadAvailableRoles(),
	]);
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
});

async function loadServiceAccount() {
	try {
		serviceAccount.value = await serviceAccountsApi.get(serviceAccountId);
		editName.value = serviceAccount.value.name;
		editDescription.value = serviceAccount.value.description || "";
		editScope.value = serviceAccount.value.scope || "ANCHOR";
		editClientIds.value = serviceAccount.value.clientIds || [];
	} catch (error) {
		console.error("Failed to fetch service account:", error);
		returnTo("/identity/service-accounts");
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
	try {
		const response = await serviceAccountsApi.getRoles(serviceAccountId);
		roleAssignments.value = response.roles;
	} catch (error) {
		console.error("Failed to fetch role assignments:", error);
	}
}

async function loadApplicationAccess() {
	try {
		const response = await usersApi.getApplicationAccess(serviceAccountId);
		appAccessGrants.value = response.applications;
		allApplications.value = response.allApplications;
	} catch (error) {
		console.error("Failed to fetch application access:", error);
	}
}

async function startAppEdit() {
	if (availableApplications.value.length === 0) {
		try {
			const response =
				await usersApi.getAvailableApplications(serviceAccountId);
			availableApplications.value = response.applications;
		} catch (error) {
			console.error("Failed to fetch available applications:", error);
		}
	}
	editAppIds.value = appAccessGrants.value.map((g) => g.applicationId);
	editAllApplications.value = allApplications.value;
	appEditMode.value = true;
}

async function saveApplicationAccess() {
	savingApps.value = true;
	try {
		// Only send the flag when it changed: turning it on needs an
		// all-applications caller, and leaving it out keeps it as is.
		await usersApi.assignApplicationAccess(
			serviceAccountId,
			editAppIds.value,
			editAllApplications.value !== allApplications.value
				? editAllApplications.value
				: undefined,
		);
		await loadApplicationAccess();
		appEditMode.value = false;
		toast.success("Success", "Application access updated");
	} catch (e: unknown) {
	} finally {
		savingApps.value = false;
	}
}

async function loadConnections() {
	loadingConnections.value = true;
	try {
		const clientScope = serviceAccount.value?.clientIds?.[0];
		const response = await connectionsApi.list(
			clientScope ? { clientId: clientScope } : {},
		);
		connections.value = response.connections.filter(
			(c) => c.serviceAccountId === serviceAccountId,
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

async function pauseConnection(id: string) {
	try {
		await connectionsApi.pause(id);
		toast.success("Success", "Connection paused");
		await loadConnections();
	} catch (e: unknown) {
	}
}

async function activateConnection(id: string) {
	try {
		await connectionsApi.activate(id);
		toast.success("Success", "Connection activated");
		await loadConnections();
	} catch (e: unknown) {
	}
}

function startEdit() {
	editName.value = serviceAccount.value?.name || "";
	editDescription.value = serviceAccount.value?.description || "";
	editScope.value = serviceAccount.value?.scope || "ANCHOR";
	editClientIds.value = serviceAccount.value?.clientIds || [];
	editMode.value = true;
}

function cancelEdit() {
	editName.value = serviceAccount.value?.name || "";
	editDescription.value = serviceAccount.value?.description || "";
	editScope.value = serviceAccount.value?.scope || "ANCHOR";
	editClientIds.value = serviceAccount.value?.clientIds || [];
	editMode.value = false;
}

async function saveServiceAccount() {
	if (!editName.value.trim()) {
		toast.error("Error", "Name is required");
		return;
	}

	saving.value = true;
	try {
		await serviceAccountsApi.update(serviceAccountId, {
			name: editName.value,
			description: editDescription.value || undefined,
			scope: editScope.value,
			// An ANCHOR account takes no clients; don't send a hidden selection.
			clientIds: editScope.value === "ANCHOR" ? [] : editClientIds.value,
		});
		// 204: refetch the stored state (the scope and links as saved).
		await loadServiceAccount();
		editMode.value = false;
		toast.success("Success", "Service account updated successfully");
	} catch (e: unknown) {
	} finally {
		saving.value = false;
	}
}

async function regenerateToken() {
	saving.value = true;
	try {
		const response = await serviceAccountsApi.regenerateToken(serviceAccountId);
		newToken.value = response.authToken;
		showRegenerateTokenDialog.value = true;
		toast.success("Success", "Auth token regenerated");
	} catch (e: unknown) {
	} finally {
		saving.value = false;
	}
}

async function regenerateSecret() {
	saving.value = true;
	try {
		const response =
			await serviceAccountsApi.regenerateSecret(serviceAccountId);
		newSecret.value = response.signingSecret;
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
	savingRoles.value = true;
	try {
		const roles = Array.from(selectedRoleNames.value);
		const response: RolesAssignedResponse =
			await serviceAccountsApi.assignRoles(serviceAccountId, roles);
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
	// Only shown below ANCHOR, where no links means no client at all.
	if (!clientIds || clientIds.length === 0) return "No client";
	return clientIds.map((id) => getClientName(id)).join(", ");
}

function formatDate(dateStr: string | null | undefined) {
	if (!dateStr) return "—";
	return new Date(dateStr).toLocaleDateString();
}

function goBack() {
	returnTo("/identity/service-accounts");
}

async function deleteServiceAccount() {
	deleting.value = true;
	try {
		await serviceAccountsApi.delete(serviceAccountId);
		toast.success("Success", "Service account deleted successfully");
		returnTo("/identity/service-accounts");
	} catch (e: unknown) {
	} finally {
		deleting.value = false;
		showDeleteDialog.value = false;
	}
}
</script>

<template>
  <div class="page-container">
    <div v-if="loading" class="loading-container">
      <ProgressSpinner strokeWidth="3" />
    </div>

    <template v-else-if="serviceAccount">
      <header class="page-header">
        <div class="header-left">
          <Button
            icon="pi pi-arrow-left"
            text
            rounded
            severity="secondary"
            @click="goBack"
            v-tooltip.right="'Back to service accounts'"
          />
          <div>
            <h1 class="page-title">{{ serviceAccount.name }}</h1>
            <p class="page-subtitle">
              <code>{{ serviceAccount.code }}</code>
            </p>
          </div>
          <Tag
            :value="serviceAccount.active ? 'Active' : 'Inactive'"
            :severity="serviceAccount.active ? 'success' : 'danger'"
          />
        </div>
        <div class="header-right">
          <Button
            label="Delete"
            icon="pi pi-trash"
            severity="danger"
            outlined
            @click="showDeleteDialog = true"
          />
        </div>
      </header>

      <!-- Service Account Information Card -->
      <div class="fc-card">
        <div class="card-header">
          <h2 class="card-title">Service Account Information</h2>
          <Button v-if="!editMode" label="Edit" icon="pi pi-pencil" text @click="startEdit" />
          <div v-else class="edit-actions">
            <Button label="Cancel" text @click="cancelEdit" />
            <Button label="Save" icon="pi pi-check" :loading="saving" @click="saveServiceAccount" />
          </div>
        </div>

        <div class="info-grid">
          <div class="info-item">
            <label>Name</label>
            <InputText v-if="editMode" v-model="editName" class="w-full" />
            <span v-else>{{ serviceAccount.name }}</span>
          </div>

          <div class="info-item">
            <label>Code</label>
            <code>{{ serviceAccount.code }}</code>
          </div>

          <div class="info-item span-2">
            <label>Description</label>
            <Textarea v-if="editMode" v-model="editDescription" rows="2" class="w-full" />
            <span v-else>{{ serviceAccount.description || '—' }}</span>
          </div>

          <div class="info-item">
            <label>Scope</label>
            <Select
              v-if="editMode"
              v-model="editScope"
              :options="scopeOptions"
              optionLabel="label"
              optionValue="value"
              class="w-full"
            />
            <Tag
              v-else
              :value="serviceAccount.scope || 'N/A'"
              :severity="
                serviceAccount.scope === 'ANCHOR'
                  ? 'success'
                  : serviceAccount.scope === 'PARTNER'
                    ? 'info'
                    : 'warn'
              "
            />
          </div>

          <div
            class="info-item span-2"
            v-if="editMode ? editScope !== 'ANCHOR' : serviceAccount.scope !== 'ANCHOR'"
          >
            <label>Client Access</label>
            <MultiSelect
              v-if="editMode"
              v-model="editClientIds"
              :options="clientOptions"
              optionLabel="label"
              optionValue="value"
              placeholder="Select clients..."
              display="chip"
              filter
              class="w-full"
            />
            <span v-else>{{ getClientNames(serviceAccount.clientIds) }}</span>
          </div>

          <div class="info-item">
            <label>Auth Type</label>
            <Tag :value="serviceAccount.authType || 'BEARER'" severity="secondary" />
          </div>

          <div class="info-item">
            <label>Created</label>
            <span>{{ formatDate(serviceAccount.createdAt) }}</span>
          </div>

          <div class="info-item">
            <label>Last Used</label>
            <span>{{ formatDate(serviceAccount.lastUsedAt) }}</span>
          </div>
        </div>
      </div>

      <!-- Webhook Credentials Card -->
      <div class="fc-card">
        <div class="card-header">
          <h2 class="card-title">Webhook Credentials</h2>
        </div>

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
      </div>

      <!-- Roles Card -->
      <div class="fc-card">
        <div class="card-header">
          <h2 class="card-title">Roles</h2>
          <Button label="Manage Roles" icon="pi pi-pencil" text @click="openRolePicker" />
        </div>

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
                :severity="assignmentSourceSeverity(data.assignmentSource)"
              />
            </template>
          </Column>
          <Column field="assignedAt" header="Assigned">
            <template #body="{ data }">
              {{ formatDate(data.assignedAt) }}
            </template>
          </Column>
        </DataTable>
      </div>

      <!-- Application Access Card -->
      <div class="fc-card">
        <div class="card-header">
          <h2 class="card-title">Application Access</h2>
          <Button
            v-if="!appEditMode"
            label="Edit"
            icon="pi pi-pencil"
            text
            @click="startAppEdit"
          />
          <div v-else class="edit-actions">
            <Button label="Cancel" text @click="appEditMode = false" />
            <Button
              label="Save"
              icon="pi pi-check"
              :loading="savingApps"
              @click="saveApplicationAccess"
            />
          </div>
        </div>

        <div class="app-access">
          <div class="all-apps-row">
            <template v-if="appEditMode">
              <ToggleSwitch v-model="editAllApplications" inputId="allApplications" />
              <label for="allApplications">All applications</label>
            </template>
            <Tag
              v-else
              :value="allApplications ? 'All applications' : 'Granted applications only'"
              :severity="allApplications ? 'warn' : 'secondary'"
            />
          </div>
          <small class="help-text">
            All applications reaches every application, including future ones. Off, the
            service account reaches only the applications granted here.
          </small>

          <MultiSelect
            v-if="appEditMode"
            v-model="editAppIds"
            :options="appOptions"
            optionLabel="label"
            optionValue="value"
            placeholder="Select applications..."
            display="chip"
            filter
            :disabled="editAllApplications"
            class="app-select"
          />
          <template v-else-if="!allApplications">
            <p v-if="appAccessGrants.length === 0" class="no-apps-notice">
              No applications granted: this service account reaches no application.
            </p>
            <DataTable v-else :value="appAccessGrants" size="small">
              <Column field="applicationName" header="Application" />
              <Column field="applicationCode" header="Code">
                <template #body="{ data }">
                  <code>{{ data.applicationCode }}</code>
                </template>
              </Column>
            </DataTable>
          </template>
        </div>
      </div>

      <!-- Connections Card -->
      <div class="fc-card">
        <div class="card-header">
          <h2 class="card-title">Connections</h2>
          <Button
            label="New Connection"
            icon="pi pi-plus"
            text
            @click="showCreateConnectionDialog = true"
          />
        </div>

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
      </div>

      <ConnectionCreateDialog
        v-model:visible="showCreateConnectionDialog"
        :service-account-id="serviceAccountId"
        :client-id="serviceAccount?.clientIds?.[0]"
        @created="onConnectionCreated"
      />

      <ConfirmDialog />
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
  </div>
</template>

<style scoped>
.loading-container {
  display: flex;
  justify-content: center;
  align-items: center;
  padding: 60px;
}

.header-left {
  display: flex;
  align-items: center;
  gap: 12px;
}

.header-right {
  display: flex;
  align-items: center;
  gap: 8px;
}

.fc-card {
  margin-bottom: 24px;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 20px;
}

.card-title {
  font-size: 16px;
  font-weight: 600;
  color: #1e293b;
  margin: 0;
}

.edit-actions {
  display: flex;
  gap: 8px;
}

.info-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 20px;
}

.info-item {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.info-item.span-2 {
  grid-column: span 2;
}

.info-item label {
  font-size: 12px;
  font-weight: 500;
  color: #64748b;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.info-item span,
.info-item code {
  font-size: 14px;
  color: #1e293b;
}

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

.w-full {
  width: 100%;
}

.app-access {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.all-apps-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.help-text {
  color: var(--text-color-secondary);
}

.app-select {
  width: 100%;
}

.no-apps-notice {
  margin: 0;
  color: var(--text-color-secondary);
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

@media (max-width: 768px) {
  .info-grid {
    grid-template-columns: 1fr;
  }

  .info-item.span-2 {
    grid-column: span 1;
  }

  .role-picker {
    flex-direction: column;
    min-height: 500px;
  }

  .role-pane {
    min-height: 200px;
  }
}
</style>
