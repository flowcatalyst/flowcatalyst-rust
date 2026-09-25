<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, watch } from "vue";
import { useRoute } from "vue-router";
import {
	emailDomainMappingsApi,
	type EmailDomainMapping,
	type ScopeType,
	type TwoFactorMethod,
} from "@/api/email-domain-mappings";
import {
	identityProvidersApi,
	type IdentityProvider,
} from "@/api/identity-providers";
import { clientsApi, type Client } from "@/api/clients";
import { getErrorMessage } from "@/utils/errors";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";
import { useDirtyForm } from "@/composables/useDirtyForm";

const emit = defineEmits<{
	changed: [];
}>();

const isEditing = ref(false);

const mapping = ref<EmailDomainMapping | null>(null);
const provider = ref<IdentityProvider | null>(null);
const providers = ref<IdentityProvider[]>([]);
const clients = ref<Client[]>([]);
const loading = ref(true);
const saving = ref(false);
const loadError = ref<string | null>(null);
const saveError = ref<string | null>(null);

const editForm = ref({
	identityProviderId: "" as string,
	scopeType: "CLIENT" as ScopeType,
	primaryClientId: null as string | null,
	requiredOidcTenantId: "" as string,
	require2fa: false,
	allowed2faMethods: [] as TwoFactorMethod[],
	rememberDeviceEnabled: false,
	rememberDeviceDays: 30,
});

const { dirty, markClean, reset: resetDirty } = useDirtyForm(() => ({
	...editForm.value,
}));

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { id, goToList } = useDrawerRoute({
	listPath: "/authentication/email-domain-mappings",
	dirty: computed(() => isEditing.value && dirty.value),
});

// 2FA only applies to internal-auth domains: the linked provider must be loaded
// and not OIDC. Hidden for OIDC-linked domains.
const show2faControls = computed(
	() => !!provider.value && provider.value.type !== "OIDC",
);

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

// Provider-move confirmation. Re-pointing a domain changes how its users
// authenticate, so the save flow detours through this dialog when the
// provider select was touched.
const showMoveDialog = ref(false);
const providerChanged = computed(
	() =>
		!!mapping.value &&
		editForm.value.identityProviderId !== mapping.value.identityProviderId,
);
const targetProvider = computed(
	() =>
		providers.value.find((p) => p.id === editForm.value.identityProviderId) ||
		null,
);
// Moving to a non-OIDC provider converts the domain's SSO-provisioned users
// back to password auth (they must reset their passwords).
const moveToInternal = computed(
	() => providerChanged.value && targetProvider.value?.type !== "OIDC",
);

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
	return true;
});

// Reactive param: the drawer instance is reused when switching between rows.
const route = useRoute();
watch(
	id,
	async (value) => {
		if (!value) return;
		await loadData(value);
		if (mapping.value && route.query["edit"] === "true") {
			startEditing();
		}
	},
	{ immediate: true },
);

async function loadData(mappingId: string) {
	loading.value = true;
	loadError.value = null;
	saveError.value = null;
	isEditing.value = false;
	resetDirty();
	try {
		const [mappingData, clientsResponse, providersResponse] = await Promise.all(
			[
				emailDomainMappingsApi.get(mappingId),
				clientsApi.list(),
				identityProvidersApi.list(),
			],
		);
		mapping.value = mappingData;
		clients.value = clientsResponse.clients;
		providers.value = providersResponse.identityProviders;
		provider.value =
			providers.value.find((p) => p.id === mappingData.identityProviderId) ||
			(await identityProvidersApi.get(mappingData.identityProviderId));

		resetEditForm();
	} catch (e) {
		loadError.value =
			e instanceof Error ? e.message : "Failed to load email domain mapping";
	} finally {
		loading.value = false;
	}
}

function resetEditForm() {
	if (mapping.value) {
		editForm.value = {
			identityProviderId: mapping.value.identityProviderId,
			// Wire values are ANCHOR | PARTNER | CLIENT; the spec types them as
			// plain string, so narrow at the form boundary.
			scopeType: mapping.value.scopeType as ScopeType,
			primaryClientId: mapping.value.primaryClientId || null,
			requiredOidcTenantId: mapping.value.requiredOidcTenantId || "",
			require2fa: mapping.value.require2fa ?? false,
			// Wire values are TOTP | EMAIL_PIN; spec types them as plain string.
			allowed2faMethods: [
				...(mapping.value.allowed2faMethods ?? []),
			] as TwoFactorMethod[],
			rememberDeviceEnabled: mapping.value.rememberDeviceEnabled ?? false,
			rememberDeviceDays: mapping.value.rememberDeviceDays ?? 30,
		};
		if (mapping.value.primaryClientId) {
			selectedPrimaryClient.value =
				clients.value.find((c) => c.id === mapping.value?.primaryClientId) ||
				null;
		} else {
			selectedPrimaryClient.value = null;
		}
	}
}

const isOidcMultiTenant = computed(() => {
	return provider.value?.oidcMultiTenant === true;
});

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

function saveChanges() {
	if (!mapping.value || !isValid.value) return;
	// A provider change is a real auth-method change — confirm before applying.
	if (providerChanged.value) {
		showMoveDialog.value = true;
		return;
	}
	void applyChanges();
}

async function applyChanges() {
	if (!mapping.value) return;
	showMoveDialog.value = false;
	saving.value = true;
	saveError.value = null;

	try {
		const mappingId = mapping.value.id;

		// Provider move first, through the dedicated use case (direction-aware
		// side effects: moving to internal converts SSO-provisioned users back
		// to password auth).
		if (providerChanged.value) {
			const moved = await emailDomainMappingsApi.moveProvider(
				mappingId,
				editForm.value.identityProviderId,
			);
			if (moved.usersReset > 0) {
				toast.success(
					"Provider changed",
					`${moved.usersReset} SSO-provisioned user${moved.usersReset === 1 ? "" : "s"} converted to password sign-in (password reset required)`,
				);
			}
		}

		const updateData: Record<string, unknown> = {
			scopeType: editForm.value.scopeType,
		};

		if (editForm.value.scopeType === "CLIENT") {
			updateData["primaryClientId"] = editForm.value.primaryClientId;
		} else if (editForm.value.scopeType === "ANCHOR") {
			updateData["primaryClientId"] = null;
		}

		// Include tenant ID (empty string clears it)
		if (isOidcMultiTenant.value) {
			updateData["requiredOidcTenantId"] =
				editForm.value.requiredOidcTenantId || "";
		}

		// 2FA settings (internal-auth domains only).
		if (show2faControls.value) {
			updateData["require2fa"] = editForm.value.require2fa;
			updateData["allowed2faMethods"] = editForm.value.require2fa
				? editForm.value.allowed2faMethods
				: [];
			updateData["rememberDeviceEnabled"] =
				editForm.value.rememberDeviceEnabled;
			updateData["rememberDeviceDays"] = editForm.value.rememberDeviceDays;
		}

		await emailDomainMappingsApi.update(mappingId, updateData);
		// PUT returns 204 No Content (empty body), so re-fetch the mapping to
		// refresh the view with the saved values.
		const refreshed = await emailDomainMappingsApi.get(mappingId);
		mapping.value = refreshed;
		provider.value =
			providers.value.find((p) => p.id === refreshed.identityProviderId) ||
			provider.value;

		// Update the selected client display
		if (refreshed.primaryClientId) {
			selectedPrimaryClient.value =
				clients.value.find((c) => c.id === refreshed.primaryClientId) || null;
		} else {
			selectedPrimaryClient.value = null;
		}

		isEditing.value = false;
		resetDirty();
		toast.success("Success", "Email domain mapping updated successfully");
		emit("changed");
	} catch (e: unknown) {
		saveError.value = getErrorMessage(e, "Failed to update mapping");
	} finally {
		saving.value = false;
	}
}

async function deleteMapping() {
	if (!mapping.value) return;

	deleteLoading.value = true;

	try {
		await emailDomainMappingsApi.delete(mapping.value.id);
		toast.success(
			"Success",
			`Email domain mapping for "${mapping.value.emailDomain}" deleted`,
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
	if (!mapping.value?.primaryClientId) return "";
	const client = clients.value.find(
		(c) => c.id === mapping.value?.primaryClientId,
	);
	return client?.name || "Unknown";
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    :title="mapping?.emailDomain || 'Email Domain Mapping'"
    :subtitle="provider ? `Identity Provider: ${provider.name}` : undefined"
    :loading="loading"
    :error="loadError"
    :dirty="isEditing && dirty"
    @close="goToList()"
  >
    <template v-if="mapping && !isEditing" #header-extra>
      <Tag :value="mapping.scopeType" :severity="getScopeTypeSeverity(mapping.scopeType)" />
    </template>

    <template v-if="mapping">
      <Message v-if="saveError" severity="error" class="save-error" :closable="true" @close="saveError = null">
        {{ saveError }}
      </Message>

      <!-- Mapping -->
      <FcFormSection title="Mapping" flat>
        <!-- View mode -->
        <div v-if="!isEditing" class="fc-detail-grid">
          <FcDetailField label="Email Domain">
            <span class="domain-value">{{ mapping.emailDomain }}</span>
          </FcDetailField>
          <FcDetailField label="Identity Provider" :value="provider?.name || 'Unknown'" />
          <FcDetailField label="Scope Type" :value="mapping.scopeType" />
          <FcDetailField
            v-if="mapping.scopeType === 'CLIENT'"
            label="Primary Client"
            :value="getPrimaryClientName()"
          />
          <FcDetailField v-if="isOidcMultiTenant" label="Required OIDC Tenant ID" span>
            <code v-if="mapping.requiredOidcTenantId" class="tenant-id">{{
              mapping.requiredOidcTenantId
            }}</code>
            <span v-else class="muted">Not set</span>
          </FcDetailField>
          <FcDetailField label="Created" :value="formatDate(mapping.createdAt)" />
          <FcDetailField label="Last Updated" :value="formatDate(mapping.updatedAt)" />
        </div>

        <!-- Edit mode -->
        <div v-else class="fc-form-grid">
          <FcDetailField label="Email Domain">
            <span class="domain-value">{{ mapping.emailDomain }}</span>
            <small class="fc-field-help">Email domain cannot be changed</small>
          </FcDetailField>
          <FcFormField
            label="Identity Provider"
            required
            help="Changing the provider moves this domain's users to a different sign-in method — you'll be asked to confirm"
          >
            <template #default="{ id: fieldId }">
              <Select
                :id="fieldId"
                v-model="editForm.identityProviderId"
                :options="providers"
                optionLabel="name"
                optionValue="id"
              />
            </template>
          </FcFormField>

          <FcFormField label="Scope Type" required>
            <template #default="{ id: fieldId }">
              <Select
                :id="fieldId"
                v-model="editForm.scopeType"
                :options="scopeTypeOptions"
                optionLabel="label"
                optionValue="value"
              >
                <template #option="slotProps">
                  <div class="type-option">
                    <span class="type-label">{{ slotProps.option.label }}</span>
                    <span class="type-description">{{ slotProps.option.description }}</span>
                  </div>
                </template>
              </Select>
            </template>
          </FcFormField>

          <FcFormField
            v-if="editForm.scopeType === 'CLIENT'"
            label="Primary Client"
            required
            help="Users from this domain will be bound to this client"
          >
            <template #default="{ id: fieldId }">
              <div class="client-select">
                <AutoComplete
                  :id="fieldId"
                  v-model="selectedPrimaryClient"
                  :suggestions="filteredClients"
                  optionLabel="name"
                  placeholder="Search for a client..."
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
            </template>
          </FcFormField>

          <FcFormField
            v-if="isOidcMultiTenant"
            label="Required OIDC Tenant ID"
            required
            span
            help="For Azure AD/Entra, enter the tenant GUID. Only users from this tenant can authenticate for this domain."
          >
            <template #default="{ id: fieldId }">
              <InputText
                :id="fieldId"
                v-model="editForm.requiredOidcTenantId"
                placeholder="e.g., 2e789bd9-a313-462a-b520-df9b586c00ed"
                :invalid="isOidcMultiTenant && !editForm.requiredOidcTenantId.trim()"
              />
            </template>
          </FcFormField>

          <Message
            v-if="editForm.scopeType === 'ANCHOR'"
            severity="info"
            :closable="false"
            class="fc-span-2"
          >
            Anchor users have platform admin access and can access all clients.
          </Message>
          <Message
            v-if="editForm.scopeType === 'PARTNER'"
            severity="info"
            :closable="false"
            class="fc-span-2"
          >
            Partner users can be granted access to multiple clients after login.
          </Message>
        </div>
      </FcFormSection>

      <!-- Two-Factor Authentication -->
      <FcFormSection v-if="show2faControls" title="Two-Factor Authentication" flat>
        <!-- View mode -->
        <div v-if="!isEditing" class="fc-detail-grid">
          <FcDetailField label="Two-Factor Authentication">
            <Tag
              :value="mapping.require2fa ? 'Required' : 'Optional'"
              :severity="mapping.require2fa ? 'success' : 'secondary'"
            />
          </FcDetailField>
          <FcDetailField v-if="mapping.require2fa" label="Allowed 2FA Methods">
            <div class="role-chips">
              <Chip
                v-for="m in mapping.allowed2faMethods"
                :key="m"
                :label="m === 'TOTP' ? 'Authenticator app' : 'Email code'"
              />
            </div>
          </FcDetailField>
          <FcDetailField
            v-if="mapping.require2fa"
            label="Remember Device"
            :value="mapping.rememberDeviceEnabled ? `Allowed (${mapping.rememberDeviceDays} days)` : 'Off'"
          />
        </div>

        <!-- Edit mode -->
        <div v-else class="fc-form-grid">
          <FcFormField
            label="Require Two-Factor Authentication"
            help="Applies to password sign-in for users of this domain. Passkey sign-in is unaffected; federated (SSO) users are never prompted."
          >
            <template #default="{ id: fieldId }">
              <div class="toggle-row">
                <ToggleSwitch :inputId="fieldId" v-model="editForm.require2fa" />
                <span class="toggle-label">{{ editForm.require2fa ? 'Required' : 'Optional' }}</span>
              </div>
            </template>
          </FcFormField>

          <FcFormField v-if="editForm.require2fa" label="Allowed 2FA Methods">
            <div class="toggle-row checkbox-group">
              <label class="checkbox-row">
                <Checkbox
                  :modelValue="editForm.allowed2faMethods.includes('TOTP')"
                  binary
                  @update:modelValue="(on: boolean) => toggle2faMethod('TOTP', on)"
                />
                Authenticator app
              </label>
              <label class="checkbox-row">
                <Checkbox
                  :modelValue="editForm.allowed2faMethods.includes('EMAIL_PIN')"
                  binary
                  @update:modelValue="(on: boolean) => toggle2faMethod('EMAIL_PIN', on)"
                />
                Email code
              </label>
            </div>
          </FcFormField>

          <Message
            v-if="editForm.require2fa && editForm.allowed2faMethods.length === 0"
            severity="warn"
            :closable="false"
            class="fc-span-2"
          >
            Select at least one method.
          </Message>

          <FcFormField v-if="editForm.require2fa" label="Allow &quot;remember this device&quot;">
            <template #default="{ id: fieldId }">
              <div class="toggle-row">
                <ToggleSwitch :inputId="fieldId" v-model="editForm.rememberDeviceEnabled" />
                <span class="toggle-label">{{ editForm.rememberDeviceEnabled ? 'Allowed' : 'Off' }}</span>
              </div>
            </template>
          </FcFormField>

          <FcFormField
            v-if="editForm.require2fa && editForm.rememberDeviceEnabled"
            label="Remember for (days)"
          >
            <template #default="{ id: fieldId }">
              <InputNumber
                :inputId="fieldId"
                v-model="editForm.rememberDeviceDays"
                :min="1"
                :max="365"
                showButtons
              />
            </template>
          </FcFormField>
        </div>
      </FcFormSection>
    </template>

    <template v-if="mapping && !loading && !loadError" #footer>
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
      <Button label="Cancel" text :disabled="deleteLoading" @click="showDeleteDialog = false" />
      <Button
        label="Delete"
        icon="pi pi-trash"
        severity="danger"
        :loading="deleteLoading"
        @click="deleteMapping"
      />
    </template>
  </Dialog>

  <!-- Provider-move Confirmation Dialog -->
  <Dialog
    v-model:visible="showMoveDialog"
    header="Change Identity Provider"
    modal
    :style="{ width: '480px' }"
  >
    <div class="dialog-content">
      <p>
        Move <strong>{{ mapping?.emailDomain }}</strong> from
        <strong>{{ provider?.name || 'its current provider' }}</strong> to
        <strong>{{ targetProvider?.name || 'the selected provider' }}</strong>?
      </p>

      <Message v-if="moveToInternal" severity="warn" :closable="false">
        Users provisioned through SSO will be converted to password sign-in:
        they must reset their password before they can log in again, and roles
        synced from the identity provider will be removed.
      </Message>
      <Message v-else severity="warn" :closable="false">
        From their next login, users on this domain will sign in through
        {{ targetProvider?.name || 'the new provider' }}. Password login for
        this domain will be disabled.
      </Message>
    </div>

    <template #footer>
      <Button label="Cancel" text :disabled="saving" @click="showMoveDialog = false" />
      <Button
        label="Move Domain"
        icon="pi pi-arrow-right-arrow-left"
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

.role-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.toggle-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.checkbox-group {
  gap: 16px;
}

.checkbox-row {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 14px;
  color: #475569;
  cursor: pointer;
}

.toggle-label {
  font-size: 14px;
  color: #475569;
}
</style>
