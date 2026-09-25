<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, onMounted } from "vue";
import {
	emailDomainMappingsApi,
	type CreateEmailDomainMappingRequest,
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

const emit = defineEmits<{
	changed: [];
}>();

const providers = ref<IdentityProvider[]>([]);
const clients = ref<Client[]>([]);
const loading = ref(false);
const dataLoading = ref(true);
const error = ref<string | null>(null);

// Form state
const form = ref({
	emailDomain: "",
	identityProviderId: null as string | null,
	scopeType: "CLIENT" as ScopeType,
	primaryClientId: null as string | null,
	requiredOidcTenantId: "" as string,
	require2fa: false,
	allowed2faMethods: [] as TwoFactorMethod[],
	rememberDeviceEnabled: false,
	rememberDeviceDays: 30,
});

// Cheap dirty check: anything typed or selected counts.
const dirty = computed(
	() =>
		form.value.emailDomain !== "" ||
		form.value.identityProviderId !== null ||
		form.value.primaryClientId !== null,
);

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { goToList, replaceToDetail } = useDrawerRoute({
	listPath: "/authentication/email-domain-mappings",
	dirty,
});

// 2FA only applies to internal-auth domains: a provider must be selected and
// it must not be OIDC. Hidden before selection and for OIDC providers.
const show2faControls = computed(
	() => !!selectedProvider.value && selectedProvider.value.type !== "OIDC",
);

function toggle2faMethod(method: TwoFactorMethod, on: boolean) {
	const set = new Set(form.value.allowed2faMethods);
	if (on) set.add(method);
	else set.delete(method);
	form.value.allowed2faMethods = [...set];
}

const isSelectedProviderMultiTenant = computed(() => {
	return selectedProvider.value?.oidcMultiTenant === true;
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
		const [providersResponse, clientsResponse] = await Promise.all([
			identityProvidersApi.list(),
			clientsApi.list(),
		]);
		providers.value = providersResponse.identityProviders;
		clients.value = clientsResponse.clients;
	} catch {
		// list-load errors surface via the global error toast
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
			require2fa: show2faControls.value ? form.value.require2fa : undefined,
			allowed2faMethods:
				show2faControls.value && form.value.require2fa
					? form.value.allowed2faMethods
					: undefined,
			rememberDeviceEnabled: show2faControls.value
				? form.value.rememberDeviceEnabled
				: undefined,
			rememberDeviceDays: show2faControls.value
				? form.value.rememberDeviceDays
				: undefined,
		};

		const created = await emailDomainMappingsApi.create(requestData);
		// `created` is `{ id }` only — see api/email-domain-mappings.ts.
		// Use the form input for the toast so we don't render "undefined".
		toast.success(
			"Success",
			`Email domain mapping for "${requestData.emailDomain}" created successfully`,
		);
		emit("changed");
		replaceToDetail(created.id);
	} catch (e: unknown) {
		error.value = getErrorMessage(e, "Failed to create mapping");
	} finally {
		loading.value = false;
	}
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    title="Create Email Domain Mapping"
    subtitle="Map an email domain to an identity provider and define user scope"
    :dirty="dirty"
    @close="goToList()"
  >
    <Message
      v-if="error"
      severity="error"
      class="error-message"
      :closable="true"
      @close="error = null"
    >
      {{ error }}
    </Message>

    <FcFormSection title="Mapping" flat>
      <div class="fc-form-grid">
        <FcFormField
          label="Email Domain"
          required
          span
          :error="form.emailDomain && !isDomainValid ? 'Please enter a valid domain name' : undefined"
          help="Users with emails from this domain will use the selected identity provider"
        >
          <template #default="{ id: fieldId }">
            <InputText
              :id="fieldId"
              v-model="form.emailDomain"
              placeholder="example.com"
              :invalid="!!(form.emailDomain && !isDomainValid)"
            />
          </template>
        </FcFormField>

        <FcFormField label="Identity Provider" required>
          <template #default="{ id: fieldId }">
            <Select
              :id="fieldId"
              v-model="selectedProvider"
              :options="providers"
              optionLabel="name"
              placeholder="Select an identity provider"
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
          </template>
        </FcFormField>

        <FcFormField label="Scope Type" required>
          <template #default="{ id: fieldId }">
            <Select
              :id="fieldId"
              v-model="form.scopeType"
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
          v-if="form.scopeType === 'CLIENT'"
          label="Primary Client"
          required
          span
          help="Users from this domain will be bound to this client"
        >
          <template #default="{ id: fieldId }">
            <div class="autocomplete-wrapper">
              <AutoComplete
                :id="fieldId"
                v-model="selectedClient"
                :suggestions="filteredClients"
                optionLabel="name"
                placeholder="Search for a client..."
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
          </template>
        </FcFormField>

        <FcFormField
          v-if="isSelectedProviderMultiTenant"
          label="Required OIDC Tenant ID"
          required
          span
          help="For Azure AD/Entra, enter the tenant GUID. Only users from this tenant can authenticate for this domain."
        >
          <template #default="{ id: fieldId }">
            <InputText
              :id="fieldId"
              v-model="form.requiredOidcTenantId"
              placeholder="e.g., 2e789bd9-a313-462a-b520-df9b586c00ed"
              :invalid="isSelectedProviderMultiTenant && !form.requiredOidcTenantId.trim()"
            />
          </template>
        </FcFormField>

        <Message
          v-if="form.scopeType === 'ANCHOR'"
          severity="info"
          :closable="false"
          class="fc-span-2"
        >
          Anchor users have platform admin access and can access all clients.
        </Message>
        <Message
          v-if="form.scopeType === 'PARTNER'"
          severity="info"
          :closable="false"
          class="fc-span-2"
        >
          Partner users can be granted access to multiple clients after login.
        </Message>
      </div>
    </FcFormSection>

    <!-- Two-factor authentication (internal-auth domains only) -->
    <FcFormSection v-if="show2faControls" title="Two-Factor Authentication" flat>
      <div class="fc-form-grid">
        <FcFormField
          label="Require Two-Factor Authentication"
          help="Applies to password sign-in for this domain. Passkey sign-in is unaffected; federated (SSO) users are never prompted."
        >
          <template #default="{ id: fieldId }">
            <div class="toggle-row">
              <ToggleSwitch :inputId="fieldId" v-model="form.require2fa" />
              <span class="toggle-label">{{ form.require2fa ? 'Required' : 'Optional' }}</span>
            </div>
          </template>
        </FcFormField>

        <FcFormField v-if="form.require2fa" label="Allowed 2FA Methods">
          <div class="toggle-row checkbox-group">
            <label class="checkbox-row">
              <Checkbox
                :modelValue="form.allowed2faMethods.includes('TOTP')"
                binary
                @update:modelValue="(on: boolean) => toggle2faMethod('TOTP', on)"
              />
              Authenticator app
            </label>
            <label class="checkbox-row">
              <Checkbox
                :modelValue="form.allowed2faMethods.includes('EMAIL_PIN')"
                binary
                @update:modelValue="(on: boolean) => toggle2faMethod('EMAIL_PIN', on)"
              />
              Email code
            </label>
          </div>
        </FcFormField>

        <Message
          v-if="form.require2fa && form.allowed2faMethods.length === 0"
          severity="warn"
          :closable="false"
          class="fc-span-2"
        >
          Select at least one method.
        </Message>

        <FcFormField v-if="form.require2fa" label="Allow &quot;remember this device&quot;">
          <template #default="{ id: fieldId }">
            <div class="toggle-row">
              <ToggleSwitch :inputId="fieldId" v-model="form.rememberDeviceEnabled" />
              <span class="toggle-label">{{ form.rememberDeviceEnabled ? 'Allowed' : 'Off' }}</span>
            </div>
          </template>
        </FcFormField>

        <FcFormField
          v-if="form.require2fa && form.rememberDeviceEnabled"
          label="Remember for (days)"
        >
          <template #default="{ id: fieldId }">
            <InputNumber
              :inputId="fieldId"
              v-model="form.rememberDeviceDays"
              :min="1"
              :max="365"
              showButtons
            />
          </template>
        </FcFormField>
      </div>
    </FcFormSection>

    <template #footer>
      <FcFormActions :bordered="false">
        <Button
          label="Cancel"
          text
          :disabled="loading"
          @click="drawer?.close()"
        />
        <Button
          label="Create Mapping"
          icon="pi pi-plus"
          :loading="loading"
          :disabled="!isValid"
          @click="createMapping"
        />
      </FcFormActions>
    </template>
  </EntityDrawer>
</template>

<style scoped>
.error-message {
  margin-bottom: 16px;
}

.autocomplete-wrapper {
  display: flex;
  gap: 8px;
  align-items: center;
}

.autocomplete-wrapper .p-autocomplete {
  flex: 1;
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
