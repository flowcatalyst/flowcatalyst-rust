<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, onMounted, watch } from "vue";
import {
	identityProvidersApi,
	type CreateIdentityProviderRequest,
	type IdentityProviderType,
	type MappingScope,
} from "@/api/identity-providers";
import { ApiError } from "@/api/client";
import { clientsApi, type Client } from "@/api/clients";
import { rolesApi, type Role } from "@/api/roles";
import { getErrorMessage } from "@/utils/errors";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";

const emit = defineEmits<{
	changed: [];
}>();

const loading = ref(false);
const error = ref<string | null>(null);

// Form state
const form = ref({
	code: "",
	name: "",
	type: "OIDC" as IdentityProviderType,
	oidcIssuerUrl: "",
	oidcClientId: "",
	oidcClientSecretRef: "",
	oidcMultiTenant: false,
	oidcIssuerPattern: "",
	allowedEmailDomains: [] as string[],
	primaryClientId: null as string | null,
	mappingScope: null as MappingScope | null,
	syncRolesFromIdp: false,
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

// A scope choice is only meaningful once there's at least one domain to
// apply it to; nothing is preselected.
const showScopeChoice = computed(() => form.value.allowedEmailDomains.length > 0);

// Role allow-list picker: [availableRoles, selectedRoles]. Which platform
// roles this provider may confer via role sync; empty = no restriction.
const allRoles = ref<Role[]>([]);
const rolePickerModel = ref<[Role[], Role[]]>([[], []]);

// Optional client linked on new/unclaimed domain mappings.
const clients = ref<Client[]>([]);
const filteredClients = ref<Client[]>([]);
const selectedClient = ref<Client | null>(null);

// Anchor forbids a primary client — clear it the moment the choice changes
// away from Client.
watch(
	() => form.value.mappingScope,
	(value) => {
		if (value !== "CLIENT") {
			form.value.primaryClientId = null;
			selectedClient.value = null;
		}
		scopeError.value = null;
	},
);
watch(
	() => form.value.primaryClientId,
	() => {
		scopeError.value = null;
	},
);

onMounted(async () => {
	try {
		const [rolesResponse, clientsResponse] = await Promise.all([
			rolesApi.list(),
			clientsApi.list(),
		]);
		allRoles.value = rolesResponse.items;
		rolePickerModel.value = [[...rolesResponse.items], []];
		clients.value = clientsResponse.clients;
	} catch {
		// list-load errors surface via the global error toast
	}
});

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

// Cheap dirty check: anything typed into the identifying fields counts.
const dirty = computed(
	() =>
		form.value.code !== "" ||
		form.value.name !== "" ||
		form.value.type !== "OIDC",
);

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { goToList, replaceToDetail } = useDrawerRoute({
	listPath: "/authentication/identity-providers",
	dirty,
});

const typeOptions = [
	{
		label: "Internal (Local)",
		value: "INTERNAL",
		description: "Internal authentication (username/password)",
	},
	{
		label: "OIDC (External)",
		value: "OIDC",
		description: "External OpenID Connect provider",
	},
];

const CODE_PATTERN = /^[a-z][a-z0-9-]*$/;

const isCodeValid = computed(() => {
	return !form.value.code || CODE_PATTERN.test(form.value.code);
});

const isValid = computed(() => {
	if (!form.value.code.trim() || !form.value.name.trim()) return false;
	if (!isCodeValid.value) return false;
	if (form.value.type === "OIDC") {
		if (!form.value.oidcClientId.trim()) return false;
		if (!form.value.oidcIssuerUrl.trim()) return false; // Always required for OIDC
	}
	if (showScopeChoice.value) {
		if (!form.value.mappingScope) return false;
		if (form.value.mappingScope === "CLIENT" && !form.value.primaryClientId) {
			return false;
		}
	}
	return true;
});

function addAllowedDomain() {
	const domain = newAllowedDomain.value.trim().toLowerCase();
	if (domain && !form.value.allowedEmailDomains.includes(domain)) {
		if (domain.match(/^[a-z0-9][a-z0-9.-]*\.[a-z]{2,}$/)) {
			form.value.allowedEmailDomains.push(domain);
			newAllowedDomain.value = "";
		} else {
			toast.error("Invalid Domain", "Please enter a valid domain name");
		}
	}
}

function removeAllowedDomain(domain: string) {
	form.value.allowedEmailDomains = form.value.allowedEmailDomains.filter(
		(d) => d !== domain,
	);
}

async function createProvider() {
	if (!isValid.value) return;

	loading.value = true;
	error.value = null;
	scopeError.value = null;

	try {
		const requestData: CreateIdentityProviderRequest = {
			code: form.value.code.trim(),
			name: form.value.name.trim(),
			type: form.value.type,
			allowedEmailDomains:
				form.value.allowedEmailDomains.length > 0
					? form.value.allowedEmailDomains
					: undefined,
			mappingScope: showScopeChoice.value
				? (form.value.mappingScope ?? undefined)
				: undefined,
			primaryClientId:
				form.value.mappingScope === "CLIENT"
					? (form.value.primaryClientId ?? undefined)
					: undefined,
			...(form.value.type === "OIDC"
				? {
						syncRolesFromIdp: form.value.syncRolesFromIdp,
						allowedRoleIds:
							rolePickerModel.value[1].length > 0
								? rolePickerModel.value[1].map((r) => r.id)
								: undefined,
					}
				: {}),
			...(form.value.type === "OIDC"
				? {
						oidcIssuerUrl:
							form.value.oidcIssuerUrl.trim() || undefined,
						oidcClientId: form.value.oidcClientId.trim(),
						oidcClientSecretRef:
							form.value.oidcClientSecretRef.trim() || undefined,
						oidcMultiTenant: form.value.oidcMultiTenant,
						oidcIssuerPattern:
							form.value.oidcIssuerPattern.trim() || undefined,
					}
				: {}),
		};

		const created = await identityProvidersApi.create(requestData, {
			// Handled inline near the scope field below — don't also fire
			// the global red banner.
			suppressGlobalErrorToast: true,
		});
		toast.success(
			"Success",
			`Identity provider "${created.name}" created successfully`,
		);
		emit("changed");
		replaceToDetail(created.id);
	} catch (e: unknown) {
		if (e instanceof ApiError && MAPPING_SCOPE_ERROR_CODES.has(e.code ?? "")) {
			scopeError.value = e.message;
		} else {
			error.value = getErrorMessage(e, "Failed to create identity provider");
		}
	} finally {
		loading.value = false;
	}
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    title="Create Identity Provider"
    subtitle="Configure a new identity provider for federated authentication"
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

    <FcFormSection title="Provider" flat>
      <div class="field-stack">
        <div class="field">
          <label for="code">Code *</label>
          <InputText
            id="code"
            v-model="form.code"
            placeholder="e.g., google, azure-ad, okta"
            class="w-full"
            :invalid="!!(form.code && !isCodeValid)"
          />
          <small v-if="form.code && !isCodeValid" class="p-error">
            Lowercase letters, numbers, and hyphens only. Must start with a letter.
          </small>
          <small v-else class="field-help"
            >A unique identifier for this provider (cannot be changed later)</small
          >
        </div>

        <div class="field">
          <label for="name">Name *</label>
          <InputText
            id="name"
            v-model="form.name"
            placeholder="e.g., Google Workspace, Azure AD"
            class="w-full"
          />
          <small class="field-help">A human-readable name for this provider</small>
        </div>

        <div class="field">
          <label for="type">Type *</label>
          <Select
            id="type"
            v-model="form.type"
            :options="typeOptions"
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
      </div>
    </FcFormSection>

    <FcFormSection v-if="form.type === 'OIDC'" title="OIDC Configuration" flat>
      <div class="field-stack">
        <div class="field checkbox-field">
          <Checkbox id="multiTenant" v-model="form.oidcMultiTenant" :binary="true" />
          <label for="multiTenant" class="checkbox-label">Multi-Tenant Mode</label>
        </div>
        <small class="field-help">
          Enable for providers like Azure AD where the issuer varies per tenant
        </small>

        <div class="field">
          <label for="issuerUrl">Issuer URL *</label>
          <InputText
            id="issuerUrl"
            v-model="form.oidcIssuerUrl"
            :placeholder="
              form.oidcMultiTenant
                ? 'https://login.microsoftonline.com/organizations/v2.0'
                : 'https://login.example.com'
            "
            class="w-full"
          />
          <small class="field-help">
            {{
              form.oidcMultiTenant
                ? 'Base URL for authorization/token endpoints (e.g., .../organizations/v2.0 or .../common/v2.0)'
                : 'The OpenID Connect issuer URL'
            }}
          </small>
        </div>

        <div v-if="form.oidcMultiTenant" class="field">
          <label for="issuerPattern">Issuer Pattern</label>
          <InputText
            id="issuerPattern"
            v-model="form.oidcIssuerPattern"
            placeholder="https://login.microsoftonline.com/{tenantId}/v2.0"
            class="w-full"
          />
          <small class="field-help">
            Pattern for validating token issuer. Use {tenantId} as placeholder. Leave empty to
            auto-derive from Issuer URL.
          </small>
        </div>

        <div class="field">
          <label for="clientId">Client ID *</label>
          <InputText
            id="clientId"
            v-model="form.oidcClientId"
            placeholder="Your OAuth client ID"
            class="w-full"
          />
        </div>

        <SecretRefInput
          v-model="form.oidcClientSecretRef"
          label="Client Secret"
          help-text="Required for confidential clients"
        />
      </div>
    </FcFormSection>

    <FcFormSection title="Email Domains" flat>
      <div class="field-stack">
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
          <div v-if="form.allowedEmailDomains.length > 0" class="domain-list">
            <Chip
              v-for="domain in form.allowedEmailDomains"
              :key="domain"
              :label="domain"
              removable
              @remove="removeAllowedDomain(domain)"
            />
          </div>
          <small class="field-help">
            Domains listed here are routed to this provider in Email Domain
            management: unknown domains get a new mapping; domains mapped to
            another provider are re-linked to this one (their existing client
            and 2FA configuration is kept).
          </small>
        </div>

        <div v-if="showScopeChoice" class="field">
          <label for="mappingScope">Domain scope *</label>
          <Select
            id="mappingScope"
            v-model="form.mappingScope"
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
            Required because domains are listed above. Every new domain
            mapping needs an explicit scope — it never falls back to Anchor.
          </small>
        </div>

        <div v-if="form.mappingScope === 'CLIENT'" class="field">
          <label for="primaryClient">Primary Client *</label>
          <div class="client-select">
            <AutoComplete
              id="primaryClient"
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
            Linked on mappings that are new or not yet linked to a primary
            client. A domain's existing client link is never overwritten.
          </small>
        </div>

      </div>
    </FcFormSection>

    <FcFormSection v-if="form.type === 'OIDC'" title="Role Sync" flat>
      <div class="field-stack">
        <div class="field checkbox-field">
          <Checkbox id="syncRoles" v-model="form.syncRolesFromIdp" :binary="true" />
          <label for="syncRoles" class="checkbox-label">Sync Roles from IDP</label>
        </div>
        <small class="field-help">
          When enabled, roles from the provider's token are synchronized at
          every login. Synced roles are filtered by the allowed roles below.
        </small>

        <div v-if="form.syncRolesFromIdp" class="field">
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
            Restrict which roles this provider may grant. Leave empty to allow
            all mapped roles.
          </small>
        </div>
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
          label="Create Identity Provider"
          icon="pi pi-plus"
          :loading="loading"
          :disabled="!isValid"
          @click="createProvider"
        />
      </FcFormActions>
    </template>
  </EntityDrawer>
</template>

<style scoped>
.error-message {
  margin-bottom: 16px;
}

.field-stack {
  display: flex;
  flex-direction: column;
  gap: 20px;
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

.domain-input {
  display: flex;
  gap: 8px;
}

.flex-grow {
  flex: 1;
}

.domain-list {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 8px;
}

.w-full {
  width: 100%;
}

.client-select {
  display: flex;
  gap: 8px;
  align-items: center;
}

.client-select .p-autocomplete {
  flex: 1;
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
