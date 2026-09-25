<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import {
	emailDomainMappingsApi,
	type EmailDomainMapping,
	type ScopeType,
} from "@/api/email-domain-mappings";
import {
	identityProvidersApi,
	type IdentityProvider,
} from "@/api/identity-providers";
import { clientsApi, type Client } from "@/api/clients";
import { rolesApi, type Role } from "@/api/roles";
import { methodLabel, type TwoFactorMethod } from "@/api/twofactor";
import { getErrorMessage } from "@/utils/errors";
import { useReturnTo } from "@/composables/useReturnTo";

const route = useRoute();
const { returnTo } = useReturnTo();

const mapping = ref<EmailDomainMapping | null>(null);
const provider = ref<IdentityProvider | null>(null);
const clients = ref<Client[]>([]);
const allRoles = ref<Role[]>([]);
const loading = ref(true);
const saving = ref(false);
const error = ref<string | null>(null);

// Role picker state: [availableRoles, selectedRoles]
const rolePickerModel = ref<[Role[], Role[]]>([[], []]);

// Edit mode
const isEditing = ref(false);
const editForm = ref({
	scopeType: "CLIENT" as ScopeType,
	primaryClientId: null as string | null,
	requiredOidcTenantId: "" as string,
	syncRolesFromIdp: false,
	require2fa: false,
	allowed2faMethods: [] as TwoFactorMethod[],
	rememberDeviceEnabled: false,
	rememberDeviceDays: 30,
});

function toggle2faMethod(method: TwoFactorMethod, on: boolean) {
	const set = new Set(editForm.value.allowed2faMethods);
	if (on) set.add(method);
	else set.delete(method);
	editForm.value.allowed2faMethods = [...set];
}

// Client autocomplete
const filteredClients = ref<Client[]>([]);
const selectedPrimaryClient = ref<Client | null>(null);

// Delete dialog
const showDeleteDialog = ref(false);
const deleteLoading = ref(false);

const scopeTypeOptions = [
	{
		label: "Anchor",
		value: "ANCHOR",
		description: "Platform admin - access to all clients",
	},
	{
		label: "Partner",
		value: "PARTNER",
		description: "Partner user - access to multiple clients",
	},
	{
		label: "Client",
		value: "CLIENT",
		description: "Client user - bound to a single client",
	},
];

const isValid = computed(() => {
	if (
		editForm.value.scopeType === "CLIENT" &&
		editForm.value.primaryClientId == null
	) {
		return false;
	}
	if (isOidcMultiTenant.value && !editForm.value.requiredOidcTenantId.trim()) {
		return false;
	}
	if (
		show2faControls.value &&
		editForm.value.require2fa &&
		editForm.value.allowed2faMethods.length === 0
	) {
		return false;
	}
	return true;
});

onMounted(async () => {
	await loadData();
});

async function loadData() {
	loading.value = true;
	error.value = null;
	try {
		const id = route.params['id'] as string;
		const [mappingData, clientsResponse, rolesResponse] = await Promise.all([
			emailDomainMappingsApi.get(id),
			clientsApi.list(),
			rolesApi.list(),
		]);
		mapping.value = mappingData;
		clients.value = clientsResponse.clients;
		allRoles.value = rolesResponse.items;

		// Load the identity provider
		provider.value = await identityProvidersApi.get(
			mappingData.identityProviderId,
		);

		resetEditForm();
	} catch (e) {
		error.value =
			e instanceof Error ? e.message : "Failed to load email domain mapping";
	} finally {
		loading.value = false;
	}
}

function resetEditForm() {
	if (mapping.value) {
		editForm.value = {
			scopeType: mapping.value.scopeType,
			primaryClientId: mapping.value.primaryClientId || null,
			requiredOidcTenantId: mapping.value.requiredOidcTenantId || "",
			syncRolesFromIdp: mapping.value.syncRolesFromIdp ?? false,
			require2fa: mapping.value.require2fa ?? false,
			allowed2faMethods: [...(mapping.value.allowed2faMethods ?? [])],
			rememberDeviceEnabled: mapping.value.rememberDeviceEnabled ?? false,
			rememberDeviceDays: mapping.value.rememberDeviceDays || 30,
		};
		if (mapping.value.primaryClientId) {
			selectedPrimaryClient.value =
				clients.value.find((c) => c.id === mapping.value?.primaryClientId) ||
				null;
		} else {
			selectedPrimaryClient.value = null;
		}

		// Set up role picker
		const allowedRoleIds = new Set(mapping.value.allowedRoleIds || []);
		const selectedRoles = allRoles.value.filter((r) =>
			allowedRoleIds.has(r.id),
		);
		const availableRoles = allRoles.value.filter(
			(r) => !allowedRoleIds.has(r.id),
		);
		rolePickerModel.value = [availableRoles, selectedRoles];
	}
}

const isOidcMultiTenant = computed(() => {
	return provider.value?.oidcMultiTenant === true;
});

const isExternalIdp = computed(() => {
	return provider.value?.type === "OIDC";
});

// 2FA only applies to internal-auth (password) domains: the linked provider
// must be loaded and must not be OIDC.
const show2faControls = computed(
	() => !!provider.value && provider.value.type !== "OIDC",
);

const showRolePicker = computed(() => {
	return isExternalIdp.value && editForm.value.scopeType !== "ANCHOR";
});

const showRoleDisplay = computed(() => {
	return isExternalIdp.value && mapping.value?.scopeType !== "ANCHOR";
});

function getAllowedRoleNames(): string[] {
	if (!mapping.value?.allowedRoleIds?.length) return [];
	return mapping.value.allowedRoleIds.map((id) => {
		const role = allRoles.value.find((r) => r.id === id);
		return role?.displayName || role?.name || id;
	});
}

function startEditing() {
	resetEditForm();
	isEditing.value = true;
}

function cancelEditing() {
	resetEditForm();
	isEditing.value = false;
}

function searchClients(event: { query: string }) {
	const query = event.query.toLowerCase();
	filteredClients.value = clients.value.filter(
		(c) =>
			c.name.toLowerCase().includes(query) ||
			c.identifier.toLowerCase().includes(query),
	);
}

function onClientSelect(event: { value: Client }) {
	editForm.value.primaryClientId = event.value.id;
}

function clearPrimaryClient() {
	editForm.value.primaryClientId = null;
	selectedPrimaryClient.value = null;
}

async function saveChanges() {
	if (!mapping.value || !isValid.value) return;

	saving.value = true;
	error.value = null;

	try {
		const updateData: Record<string, unknown> = {
			scopeType: editForm.value.scopeType,
		};

		if (editForm.value.scopeType === "CLIENT") {
			updateData['primaryClientId'] = editForm.value.primaryClientId;
		} else if (editForm.value.scopeType === "ANCHOR") {
			updateData['primaryClientId'] = null;
		}

		// Include tenant ID (empty string clears it)
		if (isOidcMultiTenant.value) {
			updateData['requiredOidcTenantId'] =
				editForm.value.requiredOidcTenantId || "";
		}

		// Include allowed roles (send the selected roles' IDs)
		if (showRolePicker.value) {
			updateData['allowedRoleIds'] = rolePickerModel.value[1].map((r) => r.id);
		}

		// Include syncRolesFromIdp for external IDPs with non-ANCHOR scope
		if (showRolePicker.value) {
			updateData['syncRolesFromIdp'] = editForm.value.syncRolesFromIdp;
		}

		// The second-factor policy (internal-auth domains only)
		if (show2faControls.value) {
			updateData['require2fa'] = editForm.value.require2fa;
			updateData['allowed2faMethods'] = editForm.value.require2fa
				? editForm.value.allowed2faMethods
				: [];
			updateData['rememberDeviceEnabled'] = editForm.value.rememberDeviceEnabled;
			updateData['rememberDeviceDays'] = editForm.value.rememberDeviceDays;
		}

		// 204 No Content: refetch the mapping rather than reading the reply.
		await emailDomainMappingsApi.update(mapping.value.id, updateData);
		mapping.value = await emailDomainMappingsApi.get(mapping.value.id);
		resetEditForm();

		isEditing.value = false;
		toast.success("Success", "Email domain mapping updated successfully");
	} catch (e: unknown) {
		error.value = getErrorMessage(e, "Failed to update mapping");
	} finally {
		saving.value = false;
	}
}

async function deleteMapping() {
	if (!mapping.value) return;

	deleteLoading.value = true;

	try {
		await emailDomainMappingsApi.delete(mapping.value.id);
		toast.success("Success", `Email domain mapping for "${mapping.value.emailDomain}" deleted`);
		returnTo("/authentication/email-domain-mappings");
	} catch (e: unknown) {
	} finally {
		deleteLoading.value = false;
		showDeleteDialog.value = false;
	}
}

function formatDate(dateString: string) {
	return new Date(dateString).toLocaleString();
}

function getScopeTypeSeverity(scopeType: string) {
	switch (scopeType) {
		case "ANCHOR":
			return "danger";
		case "PARTNER":
			return "warn";
		case "CLIENT":
			return "info";
		default:
			return "secondary";
	}
}

function getPrimaryClientName(): string {
	if (!mapping.value?.primaryClientId) return "-";
	const client = clients.value.find(
		(c) => c.id === mapping.value?.primaryClientId,
	);
	return client?.name || "Unknown";
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <Button
          icon="pi pi-arrow-left"
          text
          class="back-button"
          @click="returnTo('/authentication/email-domain-mappings')"
        />
        <h1 class="page-title">{{ mapping?.emailDomain || 'Email Domain Mapping' }}</h1>
        <p class="page-subtitle" v-if="provider">Identity Provider: {{ provider.name }}</p>
      </div>
      <div v-if="mapping && !isEditing" class="header-actions">
        <Button
          label="Delete"
          icon="pi pi-trash"
          severity="danger"
          text
          @click="showDeleteDialog = true"
        />
        <Button label="Edit" icon="pi pi-pencil" @click="startEditing" />
      </div>
    </header>

    <div v-if="loading" class="loading-container">
      <ProgressSpinner strokeWidth="3" />
    </div>

    <Message v-else-if="error" severity="error" class="error-message">{{ error }}</Message>

    <template v-else-if="mapping">
      <div class="fc-card">
        <div class="card-header">
          <h2 class="card-title">Mapping Configuration</h2>
          <div v-if="!isEditing" class="status-badges">
            <Tag :value="mapping.scopeType" :severity="getScopeTypeSeverity(mapping.scopeType)" />
          </div>
        </div>

        <div class="form-content">
          <!-- View Mode -->
          <template v-if="!isEditing">
            <div class="field-group">
              <label>Email Domain</label>
              <span class="field-value domain-value">{{ mapping.emailDomain }}</span>
            </div>

            <div class="field-group">
              <label>Identity Provider</label>
              <span class="field-value">{{ provider?.name || 'Unknown' }}</span>
            </div>

            <div class="field-group">
              <label>Scope Type</label>
              <span class="field-value">{{ mapping.scopeType }}</span>
            </div>

            <div class="field-group" v-if="mapping.scopeType === 'CLIENT'">
              <label>Primary Client</label>
              <span class="field-value">{{ getPrimaryClientName() }}</span>
            </div>

            <div class="field-group" v-if="isOidcMultiTenant">
              <label>Required OIDC Tenant ID</label>
              <span class="field-value" v-if="mapping.requiredOidcTenantId">
                <code class="tenant-id">{{ mapping.requiredOidcTenantId }}</code>
              </span>
              <span class="field-value muted" v-else>Not set</span>
            </div>

            <div class="field-group" v-if="showRoleDisplay">
              <label>Allowed Roles</label>
              <div v-if="mapping.allowedRoleIds?.length > 0" class="role-chips">
                <Chip v-for="roleName in getAllowedRoleNames()" :key="roleName" :label="roleName" />
              </div>
              <span class="field-value muted" v-else>All roles allowed</span>
            </div>

            <div class="field-group" v-if="showRoleDisplay">
              <label>Sync Roles from IDP</label>
              <span class="field-value">
                <Tag
                  :value="mapping.syncRolesFromIdp ? 'Enabled' : 'Disabled'"
                  :severity="mapping.syncRolesFromIdp ? 'success' : 'secondary'"
                />
              </span>
            </div>

            <template v-if="show2faControls">
              <div class="field-group">
                <label>Two-Factor Authentication</label>
                <span class="field-value">
                  <Tag
                    :value="mapping.require2fa ? 'Required' : 'Optional'"
                    :severity="mapping.require2fa ? 'success' : 'secondary'"
                  />
                </span>
              </div>

              <div class="field-group" v-if="mapping.require2fa">
                <label>Allowed 2FA Methods</label>
                <div v-if="mapping.allowed2faMethods?.length" class="role-chips">
                  <Chip
                    v-for="m in mapping.allowed2faMethods"
                    :key="m"
                    :label="methodLabel(m)"
                  />
                </div>
                <span class="field-value muted" v-else>None</span>
              </div>

              <div class="field-group" v-if="mapping.require2fa">
                <label>Remember Device</label>
                <span class="field-value">
                  {{ mapping.rememberDeviceEnabled ? `Allowed (${mapping.rememberDeviceDays} days)` : 'Off' }}
                </span>
              </div>
            </template>

            <div class="field-group">
              <label>Created</label>
              <span class="field-value">{{ formatDate(mapping.createdAt) }}</span>
            </div>

            <div class="field-group">
              <label>Last Updated</label>
              <span class="field-value">{{ formatDate(mapping.updatedAt) }}</span>
            </div>
          </template>

          <!-- Edit Mode -->
          <template v-else>
            <div class="field-group">
              <label>Email Domain</label>
              <span class="field-value domain-value">{{ mapping.emailDomain }}</span>
              <small class="field-help">Email domain cannot be changed</small>
            </div>

            <div class="field-group">
              <label>Identity Provider</label>
              <span class="field-value">{{ provider?.name || 'Unknown' }}</span>
              <small class="field-help">Identity provider cannot be changed</small>
            </div>

            <div class="field">
              <label for="scopeType">Scope Type *</label>
              <Select
                id="scopeType"
                v-model="editForm.scopeType"
                :options="scopeTypeOptions"
                optionLabel="label"
                optionValue="value"
                class="w-full"
              >
                <template #option="slotProps">
                  <div class="type-option">
                    <span class="type-label">{{ slotProps.option.label }}</span>
                    <span class="type-description">{{ slotProps.option.description }}</span>
                  </div>
                </template>
              </Select>
            </div>

            <div v-if="editForm.scopeType === 'CLIENT'" class="field">
              <label for="primaryClient">Primary Client *</label>
              <div class="client-select">
                <AutoComplete
                  id="primaryClient"
                  v-model="selectedPrimaryClient"
                  :suggestions="filteredClients"
                  optionLabel="name"
                  placeholder="Search for a client..."
                  class="w-full"
                  @complete="searchClients"
                  @item-select="onClientSelect"
                />
                <Button
                  v-if="selectedPrimaryClient"
                  icon="pi pi-times"
                  text
                  @click="clearPrimaryClient"
                />
              </div>
              <small class="field-help">
                Users from this domain will be bound to this client
              </small>
            </div>

            <div v-if="isOidcMultiTenant" class="field">
              <label for="requiredOidcTenantId">Required OIDC Tenant ID *</label>
              <InputText
                id="requiredOidcTenantId"
                v-model="editForm.requiredOidcTenantId"
                placeholder="e.g., 2e789bd9-a313-462a-b520-df9b586c00ed"
                class="w-full"
                :invalid="isOidcMultiTenant && !editForm.requiredOidcTenantId.trim()"
              />
              <small class="field-help">
                For Azure AD/Entra, enter the tenant GUID. Only users from this tenant can
                authenticate for this domain.
              </small>
            </div>

            <div v-if="showRolePicker" class="field">
              <label>Allowed Roles</label>
              <small class="field-help" style="margin-bottom: 8px; display: block">
                Restrict which roles users from this domain can be assigned. Move roles to the right
                to allow them. Leave empty to allow all roles.
              </small>
              <PickList
                v-model="rolePickerModel"
                dataKey="id"
                breakpoint="960px"
                :showSourceControls="false"
                :showTargetControls="false"
              >
                <template #sourceheader>Available Roles</template>
                <template #targetheader>Allowed Roles</template>
                <template #item="{ item }">
                  <div class="role-item">
                    <span class="role-name">{{ item.displayName || item.name }}</span>
                    <span class="role-app">{{ item.applicationCode }}</span>
                  </div>
                </template>
              </PickList>
            </div>

            <div v-if="showRolePicker" class="field">
              <label for="syncRolesFromIdp">Sync Roles from IDP</label>
              <div class="toggle-row">
                <ToggleSwitch id="syncRolesFromIdp" v-model="editForm.syncRolesFromIdp" />
                <span class="toggle-label">{{
                  editForm.syncRolesFromIdp ? 'Enabled' : 'Disabled'
                }}</span>
              </div>
              <small class="field-help">
                When enabled, roles from the external IDP token will be synchronized during OIDC
                login. Synced roles are filtered by the allowed roles list above.
              </small>
            </div>

            <!-- Two-factor authentication (internal-auth domains only) -->
            <template v-if="show2faControls">
              <div class="field">
                <label for="require2fa">Require Two-Factor Authentication</label>
                <div class="toggle-row">
                  <ToggleSwitch inputId="require2fa" v-model="editForm.require2fa" />
                  <span class="toggle-label">{{ editForm.require2fa ? 'Required' : 'Optional' }}</span>
                </div>
                <small class="field-help">
                  Applies to password sign-in for this domain. Passkey sign-in is unaffected;
                  federated (SSO) users are never prompted.
                </small>
              </div>

              <div v-if="editForm.require2fa" class="field">
                <label>Allowed 2FA Methods</label>
                <div class="checkbox-group">
                  <div class="checkbox-row">
                    <Checkbox
                      inputId="method-totp"
                      :modelValue="editForm.allowed2faMethods.includes('TOTP')"
                      binary
                      @update:modelValue="(on: boolean) => toggle2faMethod('TOTP', on)"
                    />
                    <label for="method-totp">Authenticator app</label>
                  </div>
                  <div class="checkbox-row">
                    <Checkbox
                      inputId="method-email"
                      :modelValue="editForm.allowed2faMethods.includes('EMAIL_PIN')"
                      binary
                      @update:modelValue="(on: boolean) => toggle2faMethod('EMAIL_PIN', on)"
                    />
                    <label for="method-email">Email code</label>
                  </div>
                </div>
              </div>

              <Message
                v-if="editForm.require2fa && editForm.allowed2faMethods.length === 0"
                severity="warn"
                :closable="false"
              >
                Select at least one method.
              </Message>

              <div v-if="editForm.require2fa" class="field">
                <label for="rememberDeviceEnabled">Allow "remember this device"</label>
                <div class="toggle-row">
                  <ToggleSwitch
                    inputId="rememberDeviceEnabled"
                    v-model="editForm.rememberDeviceEnabled"
                  />
                  <span class="toggle-label">{{
                    editForm.rememberDeviceEnabled ? 'Allowed' : 'Off'
                  }}</span>
                </div>
              </div>

              <div v-if="editForm.require2fa && editForm.rememberDeviceEnabled" class="field">
                <label for="rememberDeviceDays">Remember for (days)</label>
                <InputNumber
                  inputId="rememberDeviceDays"
                  v-model="editForm.rememberDeviceDays"
                  :min="1"
                  :max="365"
                  showButtons
                />
              </div>
            </template>

            <Message v-if="editForm.scopeType === 'ANCHOR'" severity="info" :closable="false">
              Anchor users have platform admin access and can access all clients.
            </Message>

            <Message v-if="editForm.scopeType === 'PARTNER'" severity="info" :closable="false">
              Partner users can be granted access to multiple clients after login.
            </Message>

            <div class="form-actions">
              <Button label="Cancel" text @click="cancelEditing" :disabled="saving" />
              <Button
                label="Save Changes"
                icon="pi pi-check"
                @click="saveChanges"
                :loading="saving"
                :disabled="!isValid"
              />
            </div>
          </template>
        </div>
      </div>
    </template>

    <!-- Delete Confirmation Dialog -->
    <Dialog
      v-model:visible="showDeleteDialog"
      header="Delete Email Domain Mapping"
      modal
      :style="{ width: '450px' }"
    >
      <div class="dialog-content">
        <p>
          Are you sure you want to delete the mapping for
          <strong>{{ mapping?.emailDomain }}</strong
          >?
        </p>

        <Message severity="warn" :closable="false">
          Users from this domain will no longer be able to authenticate.
        </Message>
      </div>

      <template #footer>
        <Button label="Cancel" text @click="showDeleteDialog = false" :disabled="deleteLoading" />
        <Button
          label="Delete"
          icon="pi pi-trash"
          severity="danger"
          @click="deleteMapping"
          :loading="deleteLoading"
        />
      </template>
    </Dialog>
  </div>
</template>

<style scoped>
.back-button {
  margin-right: 8px;
}

.header-actions {
  display: flex;
  gap: 8px;
}

.loading-container {
  display: flex;
  justify-content: center;
  padding: 60px;
}

.error-message {
  margin-bottom: 16px;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 24px;
  padding-bottom: 16px;
  border-bottom: 1px solid #e2e8f0;
}

.card-title {
  font-size: 18px;
  font-weight: 600;
  margin: 0;
  color: #1e293b;
}

.status-badges {
  display: flex;
  gap: 8px;
}

.form-content {
  display: flex;
  flex-direction: column;
  gap: 20px;
  max-width: 600px;
}

.field-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.field-group label {
  font-weight: 500;
  color: #64748b;
  font-size: 13px;
}

.field-value {
  color: #1e293b;
  font-size: 15px;
}

.domain-value {
  font-family: monospace;
  background: #f1f5f9;
  padding: 4px 8px;
  border-radius: 4px;
  display: inline-block;
}

.tenant-id {
  font-family: monospace;
  background: #f1f5f9;
  padding: 2px 6px;
  border-radius: 4px;
  font-size: 13px;
}

.muted {
  color: #94a3b8;
  font-style: italic;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.field label {
  font-weight: 500;
  color: #334155;
}

.field-help {
  color: #64748b;
  font-size: 12px;
}

.type-option {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 4px 0;
}

.type-option .type-label {
  font-size: 14px;
  font-weight: 500;
}

.type-option .type-description {
  font-size: 12px;
  color: #64748b;
}

.client-select {
  display: flex;
  gap: 8px;
  align-items: center;
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
  padding-top: 16px;
  border-top: 1px solid #e2e8f0;
}

.dialog-content {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.w-full {
  width: 100%;
}

.role-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.role-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 4px 0;
}

.role-item .role-name {
  font-size: 14px;
  font-weight: 500;
}

.role-item .role-app {
  font-size: 12px;
  color: #64748b;
  font-family: monospace;
}

.toggle-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.checkbox-group {
  display: flex;
  gap: 24px;
  flex-wrap: wrap;
}

.checkbox-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.field .checkbox-row label {
  font-weight: 400;
}

.toggle-label {
  font-size: 14px;
  color: #475569;
}

:deep(.p-picklist) {
  max-width: 100%;
}

:deep(.p-picklist-list) {
  min-height: 200px;
  max-height: 300px;
}
</style>
