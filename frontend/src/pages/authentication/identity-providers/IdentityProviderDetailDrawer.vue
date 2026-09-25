<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, watch } from "vue";
import { useRoute } from "vue-router";
import {
	identityProvidersApi,
	type IdentityProvider,
	type MappingScope,
} from "@/api/identity-providers";
import { ApiError } from "@/api/client";
import { rolesApi, type Role } from "@/api/roles";
import { clientsApi, type Client } from "@/api/clients";
import { getErrorMessage } from "@/utils/errors";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";
import { useDirtyForm } from "@/composables/useDirtyForm";

const emit = defineEmits<{
	changed: [];
}>();

const route = useRoute();

const isEditing = ref(false);

const provider = ref<IdentityProvider | null>(null);
const loading = ref(true);
const saving = ref(false);
const loadError = ref<string | null>(null);
const saveError = ref<string | null>(null);

// Edit mode
const editForm = ref({
	name: "",
	oidcIssuerUrl: "",
	oidcClientId: "",
	oidcClientSecretRef: "",
	oidcMultiTenant: false,
	oidcIssuerPattern: "",
	allowedEmailDomains: [] as string[],
	syncRolesFromIdp: false,
	// The provider does not store a scope — every edit starts unselected
	// (see resetEditForm). Sent only when the user makes a choice.
	mappingScope: null as MappingScope | null,
	primaryClientId: null as string | null,
});
const newAllowedDomain = ref("");

// Error codes the server returns for a missing/invalid domain-mapping scope
// choice (400s) — surfaced inline near the scope field, not a generic toast.
const MAPPING_SCOPE_ERROR_CODES = new Set([
	"MAPPING_SCOPE_REQUIRED",
	"PRIMARY_CLIENT_REQUIRED",
	"PRIMARY_CLIENT_NOT_ALLOWED",
]);
const scopeError = ref<string | null>(null);

const mappingScopeOptions = [
	{
		label: "Anchor",
		value: "ANCHOR",
		description: "Platform admin - access to all clients",
	},
	{
		label: "Client",
		value: "CLIENT",
		description: "Bound to a single client",
	},
];

// Role allow-list picker: [availableRoles, selectedRoles].
const allRoles = ref<Role[]>([]);
const rolePickerModel = ref<[Role[], Role[]]>([[], []]);

// Optional client linked on domains that don't have one yet.
const clients = ref<Client[]>([]);
const filteredClients = ref<Client[]>([]);
const selectedClient = ref<Client | null>(null);

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

function clearClient() {
	editForm.value.primaryClientId = null;
	selectedClient.value = null;
}

// A scope choice is only meaningful once there's at least one domain
// routed here.
const showScopeChoice = computed(
	() => editForm.value.allowedEmailDomains.length > 0,
);
// Domains being added that aren't already on the provider — these would
// create a NEW domain mapping, so the server requires an explicit scope.
const newlyAddedDomains = computed(() => {
	const existing = new Set(provider.value?.allowedEmailDomains || []);
	return editForm.value.allowedEmailDomains.filter((d) => !existing.has(d));
});
const scopeRequired = computed(
	() => showScopeChoice.value && newlyAddedDomains.value.length > 0,
);

// Client forbidden without Anchor, required with Client — clear the client
// the moment the choice changes away from Client, and drop any stale
// server error once the user edits either field.
watch(
	() => editForm.value.mappingScope,
	(value) => {
		if (value !== "CLIENT") {
			editForm.value.primaryClientId = null;
			selectedClient.value = null;
		}
		scopeError.value = null;
	},
);
watch(
	() => editForm.value.primaryClientId,
	() => {
		scopeError.value = null;
	},
);

const { dirty, markClean, reset: resetDirty } = useDirtyForm(() => ({
	...editForm.value,
	allowedRoleIds: rolePickerModel.value[1].map((r) => r.id),
}));

// Domains present on the provider but removed in the edit form fall back to
// internal (password) auth on save — confirmed via a dialog first.
const removedDomains = computed(() => {
	if (!provider.value) return [] as string[];
	const desired = new Set(editForm.value.allowedEmailDomains);
	return (provider.value.allowedEmailDomains || []).filter(
		(d) => !desired.has(d),
	);
});
const showReleaseDialog = ref(false);

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { id, goToList } = useDrawerRoute({
	listPath: "/authentication/identity-providers",
	dirty: computed(() => isEditing.value && dirty.value),
});

// Delete dialog
const showDeleteDialog = ref(false);
const deleteLoading = ref(false);

const isValid = computed(() => {
	if (!editForm.value.name.trim()) return false;
	if (provider.value?.type === "OIDC") {
		if (!editForm.value.oidcIssuerUrl.trim()) return false; // Always required for OIDC
		if (!editForm.value.oidcClientId.trim()) return false;
	}
	if (scopeRequired.value && !editForm.value.mappingScope) return false;
	if (
		editForm.value.mappingScope === "CLIENT" &&
		!editForm.value.primaryClientId
	) {
		return false;
	}
	return true;
});

// Reactive param: the drawer instance is reused when switching between rows.
watch(
	id,
	async (value) => {
		if (!value) return;
		await loadProvider(value);
		if (provider.value && route.query["edit"] === "true") {
			startEditing();
		}
	},
	{ immediate: true },
);

async function loadProvider(providerId: string) {
	loading.value = true;
	loadError.value = null;
	saveError.value = null;
	isEditing.value = false;
	resetDirty();
	showDeleteDialog.value = false;
	newAllowedDomain.value = "";
	try {
		const [providerData, rolesResponse, clientsResponse] = await Promise.all([
			identityProvidersApi.get(providerId),
			rolesApi.list(),
			clientsApi.list(),
		]);
		provider.value = providerData;
		allRoles.value = rolesResponse.items;
		clients.value = clientsResponse.clients;
		resetEditForm();
	} catch (e) {
		provider.value = null;
		loadError.value =
			e instanceof Error ? e.message : "Failed to load identity provider";
	} finally {
		loading.value = false;
	}
}

function resetEditForm() {
	if (provider.value) {
		editForm.value = {
			name: provider.value.name,
			oidcIssuerUrl: provider.value.oidcIssuerUrl || "",
			oidcClientId: provider.value.oidcClientId || "",
			oidcClientSecretRef: "",
			oidcMultiTenant: provider.value.oidcMultiTenant,
			oidcIssuerPattern: provider.value.oidcIssuerPattern || "",
			allowedEmailDomains: [...(provider.value.allowedEmailDomains || [])],
			syncRolesFromIdp: provider.value.syncRolesFromIdp ?? false,
			// The provider doesn't store a scope, so every (re)edit starts
			// with nothing selected — never inferred/preselected.
			mappingScope: null,
			primaryClientId: null,
		};
		selectedClient.value = null;
		scopeError.value = null;
		const allowedRoleIds = new Set(provider.value.allowedRoleIds || []);
		rolePickerModel.value = [
			allRoles.value.filter((r) => !allowedRoleIds.has(r.id)),
			allRoles.value.filter((r) => allowedRoleIds.has(r.id)),
		];
	}
}

function getAllowedRoleNames(): string[] {
	if (!provider.value?.allowedRoleIds?.length) return [];
	return provider.value.allowedRoleIds.map((roleId) => {
		const role = allRoles.value.find((r) => r.id === roleId);
		return role?.displayName || role?.name || roleId;
	});
}

function startEditing() {
	resetEditForm();
	saveError.value = null;
	isEditing.value = true;
	markClean();
}

function cancelEditing() {
	resetEditForm();
	saveError.value = null;
	isEditing.value = false;
	resetDirty();
}

function addAllowedDomain() {
	const domain = newAllowedDomain.value.trim().toLowerCase();
	if (domain && !editForm.value.allowedEmailDomains.includes(domain)) {
		if (domain.match(/^[a-z0-9][a-z0-9.-]*\.[a-z]{2,}$/)) {
			editForm.value.allowedEmailDomains.push(domain);
			newAllowedDomain.value = "";
		} else {
			toast.error("Invalid Domain", "Please enter a valid domain name");
		}
	}
}

function removeAllowedDomain(domain: string) {
	editForm.value.allowedEmailDomains =
		editForm.value.allowedEmailDomains.filter((d) => d !== domain);
}

function saveChanges() {
	if (!provider.value || !isValid.value) return;
	// Removing a domain flips its users back to password auth — confirm.
	if (removedDomains.value.length > 0) {
		showReleaseDialog.value = true;
		return;
	}
	void applyChanges();
}

async function applyChanges() {
	if (!provider.value) return;
	showReleaseDialog.value = false;
	saving.value = true;
	saveError.value = null;
	scopeError.value = null;

	try {
		const updateData: Record<string, unknown> = {
			name: editForm.value.name.trim(),
			allowedEmailDomains: editForm.value.allowedEmailDomains,
		};

		if (editForm.value.mappingScope) {
			updateData["mappingScope"] = editForm.value.mappingScope;
			if (editForm.value.mappingScope === "CLIENT") {
				updateData["primaryClientId"] = editForm.value.primaryClientId;
			}
		}

		if (provider.value.type === "OIDC") {
			updateData["oidcIssuerUrl"] = editForm.value.oidcIssuerUrl.trim() || null;
			updateData["oidcClientId"] = editForm.value.oidcClientId.trim();
			updateData["oidcMultiTenant"] = editForm.value.oidcMultiTenant;
			updateData["oidcIssuerPattern"] =
				editForm.value.oidcIssuerPattern.trim() || null;
			if (editForm.value.oidcClientSecretRef.trim()) {
				updateData["oidcClientSecretRef"] =
					editForm.value.oidcClientSecretRef.trim();
			}
			updateData["syncRolesFromIdp"] = editForm.value.syncRolesFromIdp;
			updateData["allowedRoleIds"] = rolePickerModel.value[1].map(
				(r) => r.id,
			);
		}

		const updated = await identityProvidersApi.update(
			provider.value.id,
			updateData,
			// Handled inline near the scope field below — don't also fire
			// the global red banner.
			{ suppressGlobalErrorToast: true },
		);
		provider.value = updated;
		isEditing.value = false;
		resetDirty();
		toast.success("Success", "Identity provider updated successfully");
		emit("changed");
	} catch (e: unknown) {
		if (e instanceof ApiError && MAPPING_SCOPE_ERROR_CODES.has(e.code ?? "")) {
			scopeError.value = e.message;
		} else {
			saveError.value = getErrorMessage(e, "Failed to update identity provider");
		}
	} finally {
		saving.value = false;
	}
}

async function deleteProvider() {
	if (!provider.value) return;

	deleteLoading.value = true;

	try {
		await identityProvidersApi.delete(provider.value.id);
		toast.success(
			"Success",
			`Identity provider "${provider.value.name}" deleted`,
		);
		emit("changed");
		showDeleteDialog.value = false;
		isEditing.value = false;
		void drawer.value?.close(true);
	} catch {
		showDeleteDialog.value = false;
	} finally {
		deleteLoading.value = false;
	}
}

function formatDate(dateString: string) {
	return new Date(dateString).toLocaleString();
}

function getTypeSeverity(type: string) {
	return type === "OIDC" ? "info" : "secondary";
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    :title="provider?.name || 'Identity Provider'"
    :subtitle="provider ? provider.code : undefined"
    :loading="loading"
    :error="loadError"
    :dirty="isEditing && dirty"
    @close="goToList()"
  >
    <template v-if="provider && !isEditing" #header-extra>
      <Tag :value="provider.type" :severity="getTypeSeverity(provider.type)" />
    </template>

    <template v-if="provider">
      <Message v-if="saveError" severity="error" class="save-error" :closable="true" @close="saveError = null">
        {{ saveError }}
      </Message>

      <!-- Provider -->
      <FcFormSection title="Provider" flat>
        <!-- View mode -->
        <div v-if="!isEditing" class="fc-detail-grid">
          <FcDetailField label="Name" :value="provider.name" />
          <FcDetailField label="Code">
            <code class="code-value">{{ provider.code }}</code>
          </FcDetailField>
          <FcDetailField label="Type" :value="provider.type" />
          <FcDetailField label="Created" :value="formatDate(provider.createdAt)" />
          <FcDetailField label="Last Updated" :value="formatDate(provider.updatedAt)" />
        </div>

        <!-- Edit mode -->
        <div v-else class="fc-form-grid">
          <FcDetailField label="Code">
            <code class="code-value">{{ provider.code }}</code>
            <small class="fc-field-help">Code cannot be changed</small>
          </FcDetailField>
          <FcDetailField label="Type">
            {{ provider.type }}
            <small class="fc-field-help">Type cannot be changed</small>
          </FcDetailField>

          <FcFormField label="Name" required span>
            <template #default="{ id: fieldId }">
              <InputText :id="fieldId" v-model="editForm.name" />
            </template>
          </FcFormField>
        </div>
      </FcFormSection>

      <!-- OIDC Configuration -->
      <FcFormSection v-if="provider.type === 'OIDC'" title="OIDC Configuration" flat>
        <!-- View mode -->
        <div v-if="!isEditing" class="field-stack">
          <div class="field-group">
            <label>Multi-Tenant</label>
            <span class="field-value">
              <i
                :class="
                  provider.oidcMultiTenant
                    ? 'pi pi-check text-success'
                    : 'pi pi-times text-muted'
                "
              />
              {{ provider.oidcMultiTenant ? 'Yes' : 'No' }}
            </span>
          </div>

          <div class="field-group">
            <label>Issuer URL</label>
            <span class="field-value">{{ provider.oidcIssuerUrl || '-' }}</span>
          </div>

          <div
            class="field-group"
            v-if="provider.oidcMultiTenant && provider.oidcIssuerPattern"
          >
            <label>Issuer Pattern</label>
            <span class="field-value">{{ provider.oidcIssuerPattern }}</span>
            <small class="text-muted">Auto-derived from Issuer URL if not set</small>
          </div>

          <div class="field-group">
            <label>Client ID</label>
            <span class="field-value">
              <code class="code-value">{{ provider.oidcClientId || '-' }}</code>
            </span>
          </div>

          <div class="field-group">
            <label>Client Secret</label>
            <span class="field-value">
              <i
                :class="
                  provider.hasClientSecret
                    ? 'pi pi-check text-success'
                    : 'pi pi-times text-muted'
                "
              />
              {{ provider.hasClientSecret ? 'Configured' : 'Not configured' }}
            </span>
          </div>
        </div>

        <!-- Edit mode -->
        <div v-else class="field-stack">
          <div class="field checkbox-field">
            <Checkbox id="multiTenant" v-model="editForm.oidcMultiTenant" :binary="true" />
            <label for="multiTenant" class="checkbox-label">Multi-Tenant Mode</label>
          </div>

          <div class="field">
            <label for="issuerUrl">Issuer URL *</label>
            <InputText
              id="issuerUrl"
              v-model="editForm.oidcIssuerUrl"
              :placeholder="
                editForm.oidcMultiTenant
                  ? 'https://login.microsoftonline.com/common/v2.0'
                  : 'https://login.example.com'
              "
              class="w-full"
            />
            <small class="field-help">
              {{
                editForm.oidcMultiTenant
                  ? 'Base URL for authorization/token endpoints (e.g., .../common/v2.0)'
                  : 'The OpenID Connect issuer URL'
              }}
            </small>
          </div>

          <div v-if="editForm.oidcMultiTenant" class="field">
            <label for="issuerPattern">Issuer Pattern</label>
            <InputText
              id="issuerPattern"
              v-model="editForm.oidcIssuerPattern"
              placeholder="https://login.microsoftonline.com/{tenantId}/v2.0"
              class="w-full"
            />
            <small class="field-help">
              Optional. Pattern for validating token issuer. Use {tenantId} as placeholder.
              Leave empty to auto-derive from Issuer URL.
            </small>
          </div>

          <div class="field">
            <label for="clientId">Client ID *</label>
            <InputText id="clientId" v-model="editForm.oidcClientId" class="w-full" />
          </div>

          <SecretRefInput
            v-model="editForm.oidcClientSecretRef"
            label="Client Secret"
            :help-text="
              provider.hasClientSecret
                ? 'Current secret is configured. Enter a new value to replace it, or leave blank to keep it.'
                : 'Enter the client secret'
            "
          />
        </div>
      </FcFormSection>

      <!-- Role Sync -->
      <FcFormSection v-if="provider.type === 'OIDC'" title="Role Sync" flat>
        <!-- View mode -->
        <div v-if="!isEditing" class="fc-detail-grid">
          <FcDetailField label="Sync Roles from IDP">
            <Tag
              :value="provider.syncRolesFromIdp ? 'Enabled' : 'Disabled'"
              :severity="provider.syncRolesFromIdp ? 'success' : 'secondary'"
            />
          </FcDetailField>
          <FcDetailField v-if="provider.syncRolesFromIdp" label="Allowed Roles" span>
            <div v-if="(provider.allowedRoleIds?.length ?? 0) > 0" class="role-chips">
              <Chip v-for="roleName in getAllowedRoleNames()" :key="roleName" :label="roleName" />
            </div>
            <span v-else class="text-muted">All mapped roles allowed</span>
          </FcDetailField>
        </div>

        <!-- Edit mode -->
        <div v-else class="field-stack">
          <div class="field checkbox-field">
            <Checkbox id="syncRoles" v-model="editForm.syncRolesFromIdp" :binary="true" />
            <label for="syncRoles" class="checkbox-label">Sync Roles from IDP</label>
          </div>
          <small class="field-help">
            When enabled, roles from the provider's token are synchronized at
            every login. Synced roles are filtered by the allowed roles below.
          </small>

          <div v-if="editForm.syncRolesFromIdp" class="field">
            <label>Allowed Roles</label>
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
            <small class="field-help">
              Restrict which roles this provider may grant. Leave empty to
              allow all mapped roles.
            </small>
          </div>
        </div>
      </FcFormSection>

      <!-- Email Domains -->
      <FcFormSection title="Email Domains" flat>
        <!-- View mode -->
        <template v-if="!isEditing">
          <div v-if="provider.allowedEmailDomains?.length > 0" class="domain-list">
            <Chip
              v-for="domain in provider.allowedEmailDomains"
              :key="domain"
              :label="domain"
            />
          </div>
          <span v-else class="text-muted">No domains routed to this provider</span>
        </template>

        <!-- Edit mode -->
        <div v-else class="field-stack">
          <div class="field">
            <div class="domain-input">
              <InputText
                v-model="newAllowedDomain"
                placeholder="example.com"
                class="flex-grow"
                @keyup.enter="addAllowedDomain"
              />
              <Button
                icon="pi pi-plus"
                :disabled="!newAllowedDomain.trim()"
                @click="addAllowedDomain"
              />
            </div>
            <div v-if="editForm.allowedEmailDomains.length > 0" class="domain-list">
              <Chip
                v-for="domain in editForm.allowedEmailDomains"
                :key="domain"
                :label="domain"
                removable
                @remove="removeAllowedDomain(domain)"
              />
            </div>
            <small class="field-help">
              The set of domains routed to this provider. Added domains are
              mapped (or re-linked from their current provider); removed domains
              fall back to internal password authentication.
            </small>
          </div>

          <div v-if="showScopeChoice" class="field">
            <label for="editMappingScope"
              >Domain scope<span v-if="scopeRequired"> *</span></label
            >
            <Select
              id="editMappingScope"
              v-model="editForm.mappingScope"
              :options="mappingScopeOptions"
              optionLabel="label"
              optionValue="value"
              placeholder="Choose a scope"
              class="w-full"
              :invalid="!!scopeError"
            >
              <template #option="slotProps">
                <div class="type-option">
                  <span class="type-label">{{ slotProps.option.label }}</span>
                  <span class="type-description">{{ slotProps.option.description }}</span>
                </div>
              </template>
            </Select>
            <small v-if="scopeError" class="p-error">{{ scopeError }}</small>
            <small v-else class="field-help">
              Choose Client and pick a client to link it to domains that
              don't have one yet. Existing links and scopes are not changed.
              <template v-if="scopeRequired">
                Required: a new domain was added and needs an explicit
                scope — it never falls back to Anchor.
              </template>
            </small>
          </div>

          <div v-if="editForm.mappingScope === 'CLIENT'" class="field">
            <label for="editPrimaryClient">Primary Client *</label>
            <div class="client-select">
              <AutoComplete
                id="editPrimaryClient"
                v-model="selectedClient"
                :suggestions="filteredClients"
                optionLabel="name"
                placeholder="Search for a client..."
                @complete="searchClients"
                @item-select="onClientSelect"
              />
              <Button
                v-if="selectedClient"
                icon="pi pi-times"
                text
                @click="clearClient"
              />
            </div>
            <small class="field-help">
              Linked on domains listed above that don't have a primary
              client yet. Existing client links are never overwritten.
            </small>
          </div>
        </div>
      </FcFormSection>
    </template>

    <template v-if="provider && !loading && !loadError" #footer>
      <template v-if="!isEditing">
        <Button
          label="Delete"
          icon="pi pi-trash"
          severity="danger"
          text
          @click="showDeleteDialog = true"
        />
        <Button label="Edit" icon="pi pi-pencil" @click="startEditing" />
      </template>
      <FcFormActions v-else :bordered="false">
        <Button v-if="dirty" label="Discard" text :disabled="saving" @click="cancelEditing" />
        <Button
          label="Save Changes"
          icon="pi pi-check"
          :loading="saving"
          :disabled="!dirty || !isValid"
          @click="saveChanges"
        />
      </FcFormActions>
    </template>
  </EntityDrawer>

  <!-- Delete Confirmation Dialog -->
  <Dialog
    v-model:visible="showDeleteDialog"
    header="Delete Identity Provider"
    modal
    :style="{ width: '450px' }"
  >
    <div class="dialog-content">
      <p>
        Are you sure you want to delete the identity provider
        <strong>{{ provider?.name }}</strong
        >?
      </p>

      <Message severity="warn" :closable="false">
        Deletion is blocked while email domains still route to this provider —
        move or delete those mappings first.
      </Message>
    </div>

    <template #footer>
      <Button label="Cancel" text :disabled="deleteLoading" @click="showDeleteDialog = false" />
      <Button
        label="Delete"
        icon="pi pi-trash"
        severity="danger"
        :loading="deleteLoading"
        @click="deleteProvider"
      />
    </template>
  </Dialog>

  <!-- Domain-release Confirmation Dialog -->
  <Dialog
    v-model:visible="showReleaseDialog"
    header="Remove Email Domains"
    modal
    :style="{ width: '480px' }"
  >
    <div class="dialog-content">
      <p>The following domains will stop using this provider:</p>
      <div class="domain-list">
        <Chip v-for="domain in removedDomains" :key="domain" :label="domain" />
      </div>
      <Message severity="warn" :closable="false">
        These domains fall back to internal password authentication. Users who
        were provisioned through SSO must reset their password before they can
        log in again, and roles synced from this provider will be removed.
      </Message>
    </div>

    <template #footer>
      <Button label="Cancel" text :disabled="saving" @click="showReleaseDialog = false" />
      <Button
        label="Save and Release Domains"
        icon="pi pi-check"
        severity="warn"
        :loading="saving"
        @click="applyChanges"
      />
    </template>
  </Dialog>
</template>

<style scoped>
.save-error {
  margin-bottom: 16px;
}

.code-value {
  background: #f1f5f9;
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 13px;
  font-family: monospace;
}

.field-stack {
  display: flex;
  flex-direction: column;
  gap: 20px;
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

.checkbox-field {
  flex-direction: row;
  align-items: center;
  gap: 8px;
}

.checkbox-label {
  margin: 0;
  cursor: pointer;
}

.domain-list {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.domain-input {
  display: flex;
  gap: 8px;
}

.flex-grow {
  flex: 1;
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

.client-select .p-autocomplete {
  flex: 1;
}

.dialog-content {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.text-muted {
  color: #94a3b8;
}

.text-success {
  color: #22c55e;
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

:deep(.p-picklist-list) {
  min-height: 160px;
  max-height: 240px;
}
</style>
