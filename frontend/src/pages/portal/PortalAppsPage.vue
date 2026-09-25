<script setup lang="ts">
// Portal Apps — the named portals a client runs for its customers (e.g. a
// customer portal and a supplier portal). Each app has a stable code the
// portal itself sends as `portalAppCode` when it invites users. Creating an
// app also provisions its portal OAuth client (linked to the app, PKCE,
// authorization_code only) and shows the credentials once; a login through
// it requires the user to be granted the app, and the id_token then carries
// `portal_app_code`.
import { ref, computed, onMounted, watch } from "vue";
import { useConfirm } from "primevue/useconfirm";
import { toast } from "@/utils/errorBus";
import {
	portalAppsApi,
	type PortalApp,
	type CreatePortalAppResponse,
	type PortalClientType,
} from "@/api/portal-apps";
import { clientsApi, type Client } from "@/api/clients";
import { useAuthStore } from "@/stores/auth";
import { userHasPermission } from "@/stores/permissions";
import { getErrorMessage } from "@/utils/errors";

const confirm = useConfirm();
const authStore = useAuthStore();

const clients = ref<Client[]>([]);
const selectedClientId = ref<string>("");
const apps = ref<PortalApp[]>([]);
const loading = ref(false);
// Portal users granted no portal app — locked out of app-linked portals.
const unassignedUsers = ref(0);

const isAnchor = computed(() => !authStore.user?.clientId);
const canManage = computed(() =>
	userHasPermission(authStore.user, "platform:iam:portal-user:manage"),
);

const clientOptions = computed(() => {
	if (isAnchor.value) {
		return clients.value.map((c) => ({ label: c.name, value: c.id }));
	}
	return authStore.accessibleClients.map((id) => ({
		label: clients.value.find((c) => c.id === id)?.name || id,
		value: id,
	}));
});

onMounted(async () => {
	try {
		const response = await clientsApi.list();
		clients.value = response.clients || [];
	} catch {
		// Client admins may not list clients; the picker falls back to ids.
	}
	if (!isAnchor.value) {
		selectedClientId.value =
			authStore.accessibleClients[0] ?? authStore.user?.clientId ?? "";
	} else if (clientOptions.value.length === 1) {
		selectedClientId.value = clientOptions.value[0]?.value ?? "";
	}
});

watch(selectedClientId, () => {
	if (selectedClientId.value) void loadApps();
});

async function loadApps() {
	if (!selectedClientId.value) return;
	loading.value = true;
	try {
		const response = await portalAppsApi.list(selectedClientId.value);
		apps.value = response.portalApps;
		unassignedUsers.value = response.unassignedUsers ?? 0;
	} catch (e: unknown) {
		toast.error("Error", getErrorMessage(e, "Failed to load portal apps"));
	} finally {
		loading.value = false;
	}
}

// ── Create / edit ────────────────────────────────────────────────────────

const showDialog = ref(false);
const editing = ref<PortalApp | null>(null);
const form = ref({
	code: "",
	name: "",
	description: "",
	active: true,
	// One callback URL per line — registered on the provisioned OAuth client.
	redirectUris: "",
	clientType: "CONFIDENTIAL" as PortalClientType,
});
const clientTypeOptions = [
	{ label: "Confidential — server-side portal (client secret + PKCE)", value: "CONFIDENTIAL" },
	{ label: "Public — browser-only portal (PKCE, no secret)", value: "PUBLIC" },
];

function redirectUriList() {
	return form.value.redirectUris
		.split(/\r?\n/)
		.map((u) => u.trim())
		.filter((u) => u !== "");
}
const redirectUrisValid = computed(() =>
	redirectUriList().every((u) => {
		try {
			const url = new URL(u);
			return (url.protocol === "https:" || url.protocol === "http:") && !url.host.includes("*");
		} catch {
			return false;
		}
	}),
);
const saving = ref(false);

const codeValid = computed(() =>
	/^[a-z0-9][a-z0-9_-]{0,99}$/.test(form.value.code.trim().toLowerCase()),
);
const formValid = computed(
	() =>
		form.value.name.trim() !== "" &&
		(editing.value !== null || (codeValid.value && redirectUrisValid.value)),
);

function openCreate() {
	editing.value = null;
	form.value = {
		code: "",
		name: "",
		description: "",
		active: true,
		redirectUris: "",
		clientType: "CONFIDENTIAL",
	};
	showDialog.value = true;
}

function openEdit(app: PortalApp) {
	editing.value = app;
	form.value = {
		code: app.code,
		name: app.name,
		description: app.description ?? "",
		active: app.active,
		redirectUris: "",
		clientType: "CONFIDENTIAL",
	};
	showDialog.value = true;
}

async function save() {
	if (!formValid.value || saving.value) return;
	saving.value = true;
	try {
		if (editing.value) {
			await portalAppsApi.update(editing.value.id, {
				clientId: selectedClientId.value,
				name: form.value.name.trim(),
				description: form.value.description.trim(),
				active: form.value.active,
			});
			toast.success("Success", `Portal app "${form.value.name.trim()}" updated`);
		} else {
			const result = await portalAppsApi.create({
				clientId: selectedClientId.value,
				code: form.value.code.trim().toLowerCase(),
				name: form.value.name.trim(),
				description: form.value.description.trim() || undefined,
				redirectUris: redirectUriList(),
				clientType: form.value.clientType,
			});
			toast.success("Success", `Portal app "${form.value.name.trim()}" created`);
			// Credentials hand-off: the secret is shown exactly once.
			createdWithoutCallback.value = redirectUriList().length === 0;
			created.value = result;
		}
		showDialog.value = false;
		await loadApps();
	} catch (e: unknown) {
		toast.error("Error", getErrorMessage(e, "Failed to save portal app"));
	} finally {
		saving.value = false;
	}
}

// ── Credentials hand-off (after create) ──────────────────────────────────

const created = ref<CreatePortalAppResponse | null>(null);
const createdWithoutCallback = ref(false);
const showCredentials = computed({
	get: () => created.value !== null,
	set: (open: boolean) => {
		if (!open) created.value = null;
	},
});
const origin = window.location.origin;
const endpoints = computed(() => [
	{ label: "Authorization endpoint", value: `${origin}/portal/authorize` },
	{ label: "Token endpoint", value: `${origin}/oauth/token` },
	{ label: "JWKS", value: `${origin}/.well-known/jwks.json` },
	{ label: "Issuer / discovery", value: `${origin}/.well-known/openid-configuration` },
]);
// Drop-in env for the Laravel SDK's portal mode (TS/Fastify: portal: true
// with the same client id/secret).
const envSnippet = computed(() => {
	const c = created.value;
	if (!c) return "";
	return [
		`FLOWCATALYST_BASE_URL=${origin}`,
		"FLOWCATALYST_OIDC_ENABLED=true",
		"FLOWCATALYST_OIDC_PORTAL=true",
		`FLOWCATALYST_OIDC_CLIENT_ID=${c.oauthClientId}`,
		...(c.clientSecret ? [`FLOWCATALYST_OIDC_CLIENT_SECRET=${c.clientSecret}`] : []),
		`# portalAppCode for /api/portal-users: ${c.portalApp.code}`,
	].join("\n");
});

function copy(value: string, what: string) {
	void navigator.clipboard.writeText(value);
	toast.info("Copied", `${what} copied to clipboard`);
}

// Close the gap for users with no portal app (e.g. created before portal
// apps existed): grant them all this app in one step.
function confirmAssignUnassigned(app: PortalApp) {
	confirm.require({
		message: `Grant "${app.name}" to the ${unassignedUsers.value} portal user(s) who have no portal app? Users who already have an app are not changed.`,
		header: "Assign unassigned users",
		icon: "pi pi-users",
		acceptLabel: "Assign",
		accept: async () => {
			try {
				const result = await portalAppsApi.assignUnassigned(app.id, selectedClientId.value);
				toast.success("Success", `${result.assigned} user(s) can now sign in to ${app.name}`);
				await loadApps();
			} catch (e: unknown) {
				toast.error("Error", getErrorMessage(e, "Failed to assign users"));
			}
		},
	});
}

function confirmDelete(app: PortalApp) {
	confirm.require({
		message: `Delete portal app "${app.name}"? Its OAuth client${app.oauthClients.length === 1 ? " is" : "s are"} deleted too, so the portal can no longer sign anyone in, and ${app.userCount} user(s) lose their access to it (their identities and access to other portals stay).`,
		header: "Delete portal app",
		icon: "pi pi-exclamation-triangle",
		acceptClass: "p-button-danger",
		acceptLabel: "Delete",
		accept: async () => {
			try {
				await portalAppsApi.remove(app.id, selectedClientId.value);
				toast.success("Success", `Portal app "${app.name}" deleted`);
				await loadApps();
			} catch (e: unknown) {
				toast.error("Error", getErrorMessage(e, "Failed to delete portal app"));
			}
		},
	});
}
</script>

<template>
  <div class="page-container">
    <div class="page-header">
      <div>
        <h1>Portal Apps</h1>
        <p class="page-subtitle">
          The portals a client runs for its customers. A portal sends its
          code when inviting users; users sign in only to portals they are
          granted.
        </p>
      </div>
      <Button
        v-if="canManage"
        label="New Portal App"
        icon="pi pi-plus"
        :disabled="!selectedClientId"
        @click="openCreate"
      />
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
    </div>

    <Message
      v-if="selectedClientId && unassignedUsers > 0"
      severity="warn"
      :closable="false"
      class="unassigned-banner"
    >
      {{ unassignedUsers }} portal user(s) have no portal app, so they can't sign in through
      any app-linked portal.
      <template v-if="canManage">
        Use <i class="pi pi-users" /> on an app to assign them, or grant apps one by one under
        <router-link to="/identity/portal-users">Portal Users</router-link>.
      </template>
    </Message>

    <DataTable :value="apps" :loading="loading" dataKey="id">
      <template #empty>
        <span v-if="!selectedClientId">Select a client to view its portal apps.</span>
        <span v-else>No portal apps for this client yet.</span>
      </template>
      <Column field="name" header="Name">
        <template #body="{ data }">
          <div>{{ data.name }}</div>
          <small v-if="data.description" class="text-muted">{{ data.description }}</small>
        </template>
      </Column>
      <Column field="code" header="Code">
        <template #body="{ data }"><code>{{ data.code }}</code></template>
      </Column>
      <Column header="OAuth Clients">
        <template #body="{ data }">
          <div v-if="data.oauthClients.length > 0" class="chips">
            <span v-for="oc in data.oauthClients" :key="oc.id" class="oauth-ref">
              <router-link :to="`/authentication/oauth-clients/${oc.id}`" :title="oc.clientName">
                <code>{{ oc.clientId }}</code>
              </router-link>
              <Button
                icon="pi pi-copy"
                text
                rounded
                size="small"
                title="Copy client_id"
                @click="copy(oc.clientId, 'Client ID')"
              />
            </span>
          </div>
          <span v-else class="text-muted" title="Link an OAuth client under Identity & Access → OAuth Clients">
            Not linked — no login entry yet
          </span>
        </template>
      </Column>
      <Column field="userCount" header="Users" />
      <Column field="active" header="Status">
        <template #body="{ data }">
          <Tag
            :value="data.active ? 'Active' : 'Inactive'"
            :severity="data.active ? 'success' : 'warn'"
          />
        </template>
      </Column>
      <Column v-if="canManage" header="" :style="{ width: '10rem' }">
        <template #body="{ data }">
          <div class="row-actions">
            <Button
              icon="pi pi-users"
              title="Assign users with no portal app to this app"
              text
              rounded
              :disabled="unassignedUsers === 0 || !data.active"
              @click="confirmAssignUnassigned(data)"
            />
            <Button icon="pi pi-pencil" title="Edit" text rounded @click="openEdit(data)" />
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
      v-model:visible="showDialog"
      :header="editing ? 'Edit Portal App' : 'New Portal App'"
      modal
      :style="{ width: '30rem' }"
    >
      <div class="field">
        <label for="appName">Name</label>
        <InputText id="appName" v-model="form.name" class="w-full" autofocus />
      </div>
      <div class="field">
        <label for="appCode">Code</label>
        <InputText
          id="appCode"
          v-model="form.code"
          class="w-full"
          :disabled="!!editing"
          placeholder="customer-portal"
        />
        <small class="field-help">
          <template v-if="editing">The code is fixed — the portal is configured with it.</template>
          <template v-else>
            The <code>portalAppCode</code> the portal sends. Lower-case letters, digits,
            <code>-</code> and <code>_</code>. Cannot be changed later.
          </template>
        </small>
        <small v-if="!editing && form.code && !codeValid" class="field-error">
          Invalid code.
        </small>
      </div>
      <div class="field">
        <label for="appDescription">Description (optional)</label>
        <InputText id="appDescription" v-model="form.description" class="w-full" />
      </div>
      <template v-if="!editing">
        <div class="field">
          <label for="appRedirects">Callback URL(s)</label>
          <Textarea
            id="appRedirects"
            v-model="form.redirectUris"
            rows="2"
            autoResize
            class="w-full"
            placeholder="https://portal.example.com/flowcatalyst/callback"
          />
          <small class="field-help">
            One per line — the portal's OAuth callback, registered on the OAuth client this creates.
            You can add more later under Identity &amp; Access → OAuth Clients.
          </small>
          <small v-if="!redirectUrisValid" class="field-error">
            Each callback must be an absolute http(s) URL without wildcards.
          </small>
        </div>
        <div class="field">
          <label for="appClientType">OAuth client type</label>
          <Select
            id="appClientType"
            v-model="form.clientType"
            :options="clientTypeOptions"
            optionLabel="label"
            optionValue="value"
            class="w-full"
          />
        </div>
      </template>
      <div v-if="editing" class="field checkbox-field">
        <Checkbox id="appActive" v-model="form.active" :binary="true" />
        <label for="appActive">Active — inactive portals refuse sign-ins and new grants</label>
      </div>
      <template #footer>
        <Button label="Cancel" text :disabled="saving" @click="showDialog = false" />
        <Button
          :label="editing ? 'Save' : 'Create'"
          :loading="saving"
          :disabled="!formValid"
          @click="save"
        />
      </template>
    </Dialog>

    <Dialog
      v-model:visible="showCredentials"
      header="Portal app created"
      modal
      :closable="true"
      :style="{ width: '40rem' }"
    >
      <template v-if="created">
        <p class="cred-intro">
          <strong>{{ created.portalApp.name }}</strong> is ready. Configure the portal with these
          values.
          <template v-if="created.clientSecret">
            The client secret is shown <strong>only once</strong> — store it now.
          </template>
        </p>
        <div class="cred-row">
          <span class="cred-label">Portal app code</span>
          <code class="cred-value">{{ created.portalApp.code }}</code>
          <Button icon="pi pi-copy" text rounded @click="copy(created.portalApp.code, 'Portal app code')" />
        </div>
        <div class="cred-row">
          <span class="cred-label">Client ID</span>
          <code class="cred-value">{{ created.oauthClientId }}</code>
          <Button icon="pi pi-copy" text rounded @click="copy(created.oauthClientId, 'Client ID')" />
        </div>
        <div v-if="created.clientSecret" class="cred-row">
          <span class="cred-label">Client secret</span>
          <code class="cred-value">{{ created.clientSecret }}</code>
          <Button icon="pi pi-copy" text rounded @click="copy(created.clientSecret, 'Client secret')" />
        </div>
        <div v-else class="cred-row">
          <span class="cred-label">Client secret</span>
          <span class="text-muted">None — public client (PKCE only)</span>
        </div>
        <div v-for="ep in endpoints" :key="ep.label" class="cred-row">
          <span class="cred-label">{{ ep.label }}</span>
          <code class="cred-value">{{ ep.value }}</code>
          <Button icon="pi pi-copy" text rounded @click="copy(ep.value, ep.label)" />
        </div>
        <div class="field">
          <label>Laravel SDK environment</label>
          <pre class="env-snippet">{{ envSnippet }}</pre>
          <Button
            label="Copy environment"
            icon="pi pi-copy"
            text
            size="small"
            @click="copy(envSnippet, 'Environment')"
          />
        </div>
        <Message v-if="createdWithoutCallback" severity="warn" :closable="false">
          No callback URL registered yet — add one on the
          <router-link :to="`/authentication/oauth-clients/${created.oauthClientRowId}`">OAuth client</router-link>
          before anyone can sign in.
        </Message>
      </template>
      <template #footer>
        <Button label="Done" @click="showCredentials = false" />
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
	margin-bottom: 1rem;
}
.client-select {
	min-width: 20rem;
}
.field {
	margin-bottom: 1rem;
	display: flex;
	flex-direction: column;
	gap: 0.35rem;
}
.checkbox-field {
	flex-direction: row;
	align-items: center;
	gap: 0.5rem;
}
.field-help {
	color: var(--p-text-muted-color);
}
.field-error {
	color: var(--p-red-500);
}
.chips {
	display: flex;
	flex-wrap: wrap;
	gap: 0.25rem;
}
.row-actions {
	display: flex;
	gap: 0.25rem;
}
.text-muted {
	color: var(--p-text-muted-color);
}
.unassigned-banner {
	margin-bottom: 1rem;
}
.oauth-ref {
	display: inline-flex;
	align-items: center;
	gap: 0.1rem;
}
.cred-intro {
	margin-top: 0;
}
.cred-row {
	display: grid;
	grid-template-columns: 11rem 1fr auto;
	align-items: center;
	gap: 0.5rem;
	margin-bottom: 0.35rem;
}
.cred-label {
	color: var(--p-text-muted-color);
	font-size: 0.9rem;
}
.cred-value {
	overflow-wrap: anywhere;
}
.env-snippet {
	margin: 0;
	padding: 0.75rem;
	border-radius: 6px;
	background: var(--p-content-hover-background);
	overflow-x: auto;
	font-size: 0.85rem;
}
</style>
