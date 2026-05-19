<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, onMounted } from "vue";
import { useRouter } from "vue-router";
import {
	emailDomainMappingsApi,
	type CreateEmailDomainMappingRequest,
	type ScopeType,
} from "@/api/email-domain-mappings";
import {
	identityProvidersApi,
	type IdentityProvider,
} from "@/api/identity-providers";
import { clientsApi, type Client } from "@/api/clients";
import { rolesApi, type Role } from "@/api/roles";
import { getErrorMessage } from "@/utils/errors";

const router = useRouter();

const providers = ref<IdentityProvider[]>([]);
const clients = ref<Client[]>([]);
const allRoles = ref<Role[]>([]);
const loading = ref(false);
const dataLoading = ref(true);
const error = ref<string | null>(null);

// Role picker state: [availableRoles, selectedRoles]
const rolePickerModel = ref<[Role[], Role[]]>([[], []]);

// Form state
const form = ref({
	emailDomain: "",
	identityProviderId: null as string | null,
	scopeType: "CLIENT" as ScopeType,
	primaryClientId: null as string | null,
	requiredOidcTenantId: "" as string,
	syncRolesFromIdp: false,
});

const isSelectedProviderMultiTenant = computed(() => {
	return selectedProvider.value?.oidcMultiTenant === true;
});

const isExternalIdp = computed(() => {
	return selectedProvider.value?.type === "OIDC";
});

const showRolePicker = computed(() => {
	return isExternalIdp.value && form.value.scopeType !== "ANCHOR";
});

// Selection state
const selectedProvider = ref<IdentityProvider | null>(null);
const filteredClients = ref<Client[]>([]);
const selectedClient = ref<Client | null>(null);

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

const DOMAIN_PATTERN = /^[a-z0-9][a-z0-9.-]*\.[a-z]{2,}$/;

const isDomainValid = computed(() => {
	return (
		!form.value.emailDomain ||
		DOMAIN_PATTERN.test(form.value.emailDomain.toLowerCase())
	);
});

const isValid = computed(() => {
	if (!form.value.emailDomain.trim() || !isDomainValid.value) return false;
	if (!form.value.identityProviderId) return false;
	if (form.value.scopeType === "CLIENT" && !form.value.primaryClientId)
		return false;
	if (
		isSelectedProviderMultiTenant.value &&
		!form.value.requiredOidcTenantId.trim()
	)
		return false;
	return true;
});

onMounted(async () => {
	await loadData();
});

async function loadData() {
	dataLoading.value = true;
	try {
		const [providersResponse, clientsResponse, rolesResponse] =
			await Promise.all([
				identityProvidersApi.list(),
				clientsApi.list(),
				rolesApi.list(),
			]);
		providers.value = providersResponse.identityProviders;
		clients.value = clientsResponse.clients;
		allRoles.value = rolesResponse.items;
		// Initialize role picker with all roles available, none selected
		rolePickerModel.value = [[...rolesResponse.items], []];
	} catch (e: unknown) {
	} finally {
		dataLoading.value = false;
	}
}

function onProviderChange() {
	form.value.identityProviderId = selectedProvider.value?.id || null;
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
	form.value.primaryClientId = event.value.id;
}

function clearClient() {
	form.value.primaryClientId = null;
	selectedClient.value = null;
}

async function createMapping() {
	if (!isValid.value) return;

	loading.value = true;
	error.value = null;

	try {
		const requestData: CreateEmailDomainMappingRequest = {
			emailDomain: form.value.emailDomain.trim().toLowerCase(),
			identityProviderId: form.value.identityProviderId!,
			scopeType: form.value.scopeType,
			primaryClientId:
				form.value.scopeType === "CLIENT"
					? (form.value.primaryClientId ?? undefined)
					: undefined,
			requiredOidcTenantId:
				isSelectedProviderMultiTenant.value &&
				form.value.requiredOidcTenantId.trim()
					? form.value.requiredOidcTenantId.trim()
					: undefined,
			allowedRoleIds:
				showRolePicker.value && rolePickerModel.value[1].length > 0
					? rolePickerModel.value[1].map((r) => r.id)
					: undefined,
			syncRolesFromIdp: showRolePicker.value
				? form.value.syncRolesFromIdp
				: undefined,
		};

		const created = await emailDomainMappingsApi.create(requestData);
		// `created` is `{ id }` only — see api/email-domain-mappings.ts.
		// Use the form input for the toast so we don't render "undefined".
		toast.success(
			"Success",
			`Email domain mapping for "${requestData.emailDomain}" created successfully`,
		);
		router.push(`/authentication/email-domain-mappings/${created.id}`);
	} catch (e: unknown) {
		error.value = getErrorMessage(e, "Failed to create mapping");
	} finally {
		loading.value = false;
	}
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
          @click="router.push('/authentication/email-domain-mappings')"
        />
        <h1 class="page-title">Create Email Domain Mapping</h1>
        <p class="page-subtitle">
          Map an email domain to an identity provider and define user scope.
        </p>
      </div>
    </header>

    <Message
      v-if="error"
      severity="error"
      class="error-message"
      :closable="true"
      @close="error = null"
    >
      {{ error }}
    </Message>

    <div class="fc-card">
      <div class="form-content">
        <div class="field">
          <label for="emailDomain">Email Domain *</label>
          <InputText
            id="emailDomain"
            v-model="form.emailDomain"
            placeholder="example.com"
            class="w-full"
            :invalid="!!(form.emailDomain && !isDomainValid)"
          />
          <small v-if="form.emailDomain && !isDomainValid" class="p-error">
            Please enter a valid domain name
          </small>
          <small v-else class="field-help">
            Users with emails from this domain will use the selected identity provider
          </small>
        </div>

        <div class="field">
          <label for="provider">Identity Provider *</label>
          <Select
            id="provider"
            v-model="selectedProvider"
            :options="providers"
            optionLabel="name"
            placeholder="Select an identity provider"
            class="w-full"
            :loading="dataLoading"
            @change="onProviderChange"
          >
            <template #option="slotProps">
              <div class="provider-option">
                <span class="provider-name">{{ slotProps.option.name }}</span>
                <span class="provider-code">{{ slotProps.option.code }}</span>
              </div>
            </template>
          </Select>
        </div>

        <div class="field">
          <label for="scopeType">Scope Type *</label>
          <Select
            id="scopeType"
            v-model="form.scopeType"
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

        <div v-if="form.scopeType === 'CLIENT'" class="field">
          <label for="primaryClient">Primary Client *</label>
          <div class="autocomplete-wrapper">
            <AutoComplete
              id="primaryClient"
              v-model="selectedClient"
              :suggestions="filteredClients"
              optionLabel="name"
              placeholder="Search for a client..."
              class="w-full"
              :loading="dataLoading"
              @complete="searchClients"
              @item-select="onClientSelect"
            >
              <template #option="slotProps">
                <div class="client-option">
                  <span class="client-name">{{ slotProps.option.name }}</span>
                  <span class="client-identifier">{{ slotProps.option.identifier }}</span>
                </div>
              </template>
            </AutoComplete>
            <Button v-if="selectedClient" icon="pi pi-times" text @click="clearClient" />
          </div>
          <small class="field-help"> Users from this domain will be bound to this client </small>
        </div>

        <div v-if="isSelectedProviderMultiTenant" class="field">
          <label for="requiredOidcTenantId">Required OIDC Tenant ID *</label>
          <InputText
            id="requiredOidcTenantId"
            v-model="form.requiredOidcTenantId"
            placeholder="e.g., 2e789bd9-a313-462a-b520-df9b586c00ed"
            class="w-full"
            :invalid="isSelectedProviderMultiTenant && !form.requiredOidcTenantId.trim()"
          />
          <small class="field-help">
            For Azure AD/Entra, enter the tenant GUID. Only users from this tenant can authenticate
            for this domain.
          </small>
        </div>

        <div v-if="showRolePicker" class="field">
          <label>Allowed Roles</label>
          <small class="field-help" style="margin-bottom: 8px; display: block">
            Restrict which roles users from this domain can be assigned. Move roles to the right to
            allow them. Leave empty to allow all roles.
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
            <ToggleSwitch id="syncRolesFromIdp" v-model="form.syncRolesFromIdp" />
            <span class="toggle-label">{{ form.syncRolesFromIdp ? 'Enabled' : 'Disabled' }}</span>
          </div>
          <small class="field-help">
            When enabled, roles from the external IDP token will be synchronized during OIDC login.
            Synced roles are filtered by the allowed roles list above.
          </small>
        </div>

        <Message v-if="form.scopeType === 'ANCHOR'" severity="info" :closable="false">
          Anchor users have platform admin access and can access all clients.
        </Message>

        <Message v-if="form.scopeType === 'PARTNER'" severity="info" :closable="false">
          Partner users can be granted access to multiple clients after login.
        </Message>

        <div class="form-actions">
          <Button
            label="Cancel"
            text
            @click="router.push('/authentication/email-domain-mappings')"
            :disabled="loading"
          />
          <Button
            label="Create Mapping"
            icon="pi pi-plus"
            @click="createMapping"
            :loading="loading"
            :disabled="!isValid"
          />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.back-button {
  margin-right: 8px;
}

.error-message {
  margin-bottom: 16px;
}

.form-content {
  display: flex;
  flex-direction: column;
  gap: 20px;
  max-width: 600px;
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

.autocomplete-wrapper {
  display: flex;
  gap: 8px;
  align-items: center;
}

.provider-option,
.client-option {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 4px 0;
}

.provider-name,
.client-name {
  font-size: 14px;
  font-weight: 500;
}

.provider-code,
.client-identifier {
  font-size: 12px;
  color: #64748b;
  font-family: monospace;
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

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
  padding-top: 16px;
  border-top: 1px solid #e2e8f0;
}

.w-full {
  width: 100%;
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
