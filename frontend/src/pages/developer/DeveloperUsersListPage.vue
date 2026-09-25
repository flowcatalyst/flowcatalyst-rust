<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useConfirm } from "primevue/useconfirm";
import { toast } from "@/utils/errorBus";
import { usersApi, type User } from "@/api/users";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";

// Matches internal/platform/seed/roles.go's seeded "platform:developer" role
// and the literal duplicated in the token endpoint / use cases.
const DEVELOPER_ROLE = "platform:developer";

const confirm = useConfirm();

const developers = ref<User[]>([]);
const loading = ref(true);

const listState = useListState({
	filters: {
		q: { type: "string", key: "q" },
	},
});
const { filters } = listState;
const { filters: tableFilters, clearAll } = useTableFilters(listState, []);

onMounted(loadDevelopers);

async function loadDevelopers() {
	loading.value = true;
	try {
		const response = await usersApi.listDeveloperUsers();
		developers.value = response.principals;
	} catch {
	} finally {
		loading.value = false;
	}
}

function formatDate(dateStr: string | undefined | null) {
	if (!dateStr) return "—";
	return new Date(dateStr).toLocaleString();
}

// ── Grant developer role (designate an existing user) ────────────────────

const showGrantDialog = ref(false);
const userSuggestions = ref<User[]>([]);
const granting = ref(false);

function openGrantDialog() {
	userSuggestions.value = [];
	showGrantDialog.value = true;
}

async function searchUsers(event: { query: string }) {
	try {
		const response = await usersApi.list({
			q: event.query,
			type: "USER",
			pageSize: 15,
		});
		const already = new Set(developers.value.map((d) => d.id));
		userSuggestions.value = response.principals.filter(
			(p) => !already.has(p.id),
		);
	} catch {
		userSuggestions.value = [];
	}
}

function initials(name: string): string {
	const parts = name.trim().split(/\s+/);
	const first = parts[0]?.[0] ?? "";
	const last = parts.length > 1 ? (parts[parts.length - 1]?.[0] ?? "") : "";
	return (first + last).toUpperCase() || "?";
}

async function onUserSelect(event: { value: User }) {
	const user = event.value;
	granting.value = true;
	try {
		const roles = user.roles.includes(DEVELOPER_ROLE)
			? user.roles
			: [...user.roles, DEVELOPER_ROLE];
		await usersApi.assignRoles(user.id, roles);
		toast.success(
			"Granted",
			`${user.name} can now hold a developer API credential`,
		);
		showGrantDialog.value = false;
		await loadDevelopers();
	} catch {
	} finally {
		granting.value = false;
	}
}

function confirmRemoveRole(user: User) {
	confirm.require({
		message: `Remove the developer role from "${user.name}"? Their API credential (if set) will be revoked too — re-granting the role later will require setting a fresh one.`,
		header: "Remove Developer Role",
		icon: "pi pi-exclamation-triangle",
		acceptClass: "p-button-danger",
		accept: () => removeRole(user),
	});
}

// Removing the role alone would leave dev_client_secret_ref in place: if the
// role is re-granted later, the OLD secret would silently start working
// again with no re-set required. Revoking the credential here (best-effort —
// a failure shouldn't block the role removal) keeps "no longer a developer"
// and "no live credential" in sync.
async function removeRole(user: User) {
	try {
		await usersApi.removeRole(user.id, DEVELOPER_ROLE);
		if (user.hasDeveloperCredential) {
			try {
				await usersApi.revokeDeveloperCredential(user.id);
			} catch {
			}
		}
		toast.success("Removed", `Developer role removed from ${user.name}`);
		await loadDevelopers();
	} catch {
	}
}

// ── Set / rotate / revoke credential ──────────────────────────────────────

const showSecretDialog = ref(false);
const newSecret = ref<string | null>(null);
const secretForUser = ref<User | null>(null);
const busyUserId = ref<string | null>(null);

async function setSecret(user: User) {
	busyUserId.value = user.id;
	const wasSet = user.hasDeveloperCredential;
	try {
		const response = await usersApi.setDeveloperCredential(user.id);
		newSecret.value = response.clientSecret ?? null;
		secretForUser.value = user;
		showSecretDialog.value = true;
		toast.success(
			"Success",
			`Developer credential ${wasSet ? "rotated" : "set"} for ${user.name}`,
		);
		await loadDevelopers();
	} catch {
	} finally {
		busyUserId.value = null;
	}
}

function confirmRevokeSecret(user: User) {
	confirm.require({
		message: `Revoke the developer API credential for "${user.name}"? Any scripts using it will stop working immediately.`,
		header: "Revoke Credential",
		icon: "pi pi-exclamation-triangle",
		acceptClass: "p-button-danger",
		accept: () => revokeSecret(user),
	});
}

async function revokeSecret(user: User) {
	try {
		await usersApi.revokeDeveloperCredential(user.id);
		toast.success("Revoked", `Developer credential revoked for ${user.name}`);
		await loadDevelopers();
	} catch {
	}
}

function copyToClipboard(text: string) {
	navigator.clipboard.writeText(text);
	toast.info("Copied", "Client secret copied to clipboard");
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Developer Users</h1>
        <p class="page-subtitle">
          Users who can mint a self-service API credential (client_credentials, as themselves) for local testing against a deployed environment
        </p>
      </div>
      <Button label="Grant Developer Role" icon="pi pi-plus" @click="openGrantDialog" />
    </header>

    <div class="fc-card table-card">
      <DataTable
        :value="developers"
        :loading="loading"
        :filters="tableFilters"
        :globalFilterFields="['name', 'email']"
        :paginator="true"
        :rows="100"
        :rowsPerPageOptions="[50, 100, 250, 500]"
        :showCurrentPageReport="true"
        currentPageReportTemplate="Showing {first} to {last} of {totalRecords} developer users"
        stripedRows
        size="small"
      >
        <template #header>
          <FcTableToolbar
            v-model:search="filters.q.value"
            search-placeholder="Search by name or email..."
            :has-active-filters="listState.hasActiveFilters.value"
            @clear-all="clearAll"
          />
        </template>

        <Column field="name" header="Name" sortable style="width: 22%">
          <template #body="{ data }">
            <span class="user-name">{{ data.name }}</span>
          </template>
        </Column>

        <Column field="email" header="Email" sortable style="width: 25%">
          <template #body="{ data }">
            <span>{{ data.email || "—" }}</span>
          </template>
        </Column>

        <Column header="Credential" style="width: 15%">
          <template #body="{ data }">
            <Tag
              :value="data.hasDeveloperCredential ? 'Set' : 'Not set'"
              :severity="data.hasDeveloperCredential ? 'success' : 'secondary'"
            />
          </template>
        </Column>

        <Column header="Last Rotated" style="width: 18%">
          <template #body="{ data }">
            <span class="date-text">{{ formatDate(data.developerCredentialUpdatedAt) }}</span>
          </template>
        </Column>

        <Column header="Actions" style="width: 20%">
          <template #body="{ data }">
            <div class="action-buttons">
              <Button
                v-tooltip.top="data.hasDeveloperCredential ? 'Rotate Secret' : 'Set Secret'"
                icon="pi pi-key"
                text
                rounded
                severity="secondary"
                :loading="busyUserId === data.id"
                @click="setSecret(data)"
              />
              <Button
                v-if="data.hasDeveloperCredential"
                v-tooltip.top="'Revoke Credential'"
                icon="pi pi-ban"
                text
                rounded
                severity="warn"
                @click="confirmRevokeSecret(data)"
              />
              <Button
                v-tooltip.top="'Remove Developer Role'"
                icon="pi pi-user-minus"
                text
                rounded
                severity="danger"
                @click="confirmRemoveRole(data)"
              />
            </div>
          </template>
        </Column>

        <template #empty>
          <div class="empty-message">
            <i class="pi pi-code"></i>
            <span>No developer users yet</span>
            <Button label="Grant Developer Role" link @click="openGrantDialog" />
          </div>
        </template>
      </DataTable>
    </div>

    <!-- Grant Developer Role Dialog -->
    <Dialog
      v-model:visible="showGrantDialog"
      header="Grant Developer Role"
      :style="{ width: '560px' }"
      :modal="true"
      class="grant-dialog"
    >
      <p class="dialog-help">
        Search for an existing user to grant the developer role. They'll be able to set their own API credential from their Profile page, or you can set one for them below.
      </p>
      <AutoComplete
        :suggestions="userSuggestions"
        optionLabel="name"
        placeholder="Search by name or email..."
        :loading="granting"
        class="full-width grant-search"
        panelClass="grant-search-panel"
        @complete="searchUsers"
        @item-select="onUserSelect"
      >
        <template #option="slotProps">
          <div class="user-option">
            <div class="user-option-avatar">{{ initials(slotProps.option.name) }}</div>
            <div class="user-option-text">
              <span class="user-option-name">{{ slotProps.option.name }}</span>
              <span class="user-option-email">{{ slotProps.option.email }}</span>
            </div>
          </div>
        </template>
        <template #empty>
          <div class="user-option-empty">No matching users</div>
        </template>
      </AutoComplete>
      <template #footer>
        <Button label="Close" text @click="showGrantDialog = false" />
      </template>
    </Dialog>

    <!-- One-time Secret Reveal Dialog -->
    <Dialog
      v-model:visible="showSecretDialog"
      header="Developer Client Secret"
      :style="{ width: '540px' }"
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
            v-tooltip.top="'Copy'"
            @click="copyToClipboard(newSecret!)"
          />
        </div>
        <p v-if="secretForUser" class="curl-hint">
          <code>client_id={{ secretForUser.id }}</code>
        </p>
      </div>
      <template #footer>
        <Button label="Done" @click="showSecretDialog = false" />
      </template>
    </Dialog>
  </div>
</template>

<style scoped>
.table-card {
  padding: 0;
  overflow: hidden;
}

.user-name {
  font-weight: 500;
  color: #1e293b;
}

.date-text {
  font-size: 13px;
  color: #64748b;
}

.action-buttons {
  display: flex;
  gap: 4px;
}

.empty-message {
  text-align: center;
  padding: 48px 24px;
  color: #64748b;
}

.empty-message i {
  font-size: 48px;
  display: block;
  margin-bottom: 16px;
  color: #cbd5e1;
}

.empty-message span {
  display: block;
  margin-bottom: 12px;
}

:deep(.p-datatable .p-datatable-thead > tr > th) {
  background: #f8fafc;
  color: #475569;
  font-weight: 600;
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.dialog-help {
  margin: 0 0 16px;
  font-size: 13px;
  color: #64748b;
  line-height: 1.5;
}

.full-width {
  width: 100%;
}

/* PrimeVue 4's AutoComplete inner <input> doesn't inherit the root's width
   on its own — same fix as ClientSelect.vue / main.css's .fc-form-field
   rule, applied here since this dialog isn't inside an .fc-form-field. */
.grant-search :deep(.p-autocomplete-input) {
  width: 100%;
}

.grant-search :deep(.p-autocomplete-input) {
  height: 44px;
  font-size: 14px;
}

.user-option {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 4px 0;
}

.user-option-avatar {
  flex-shrink: 0;
  width: 32px;
  height: 32px;
  border-radius: 50%;
  background: linear-gradient(135deg, #0967d2 0%, #47a3f3 100%);
  color: white;
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: 600;
  font-size: 12px;
}

.user-option-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}

.user-option-name {
  font-weight: 500;
}

.user-option-email {
  font-size: 12px;
  color: #64748b;
}

.user-option-empty {
  padding: 10px 4px;
  font-size: 13px;
  color: #64748b;
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

.curl-hint {
  margin: 0;
  font-size: 12px;
  color: #64748b;
}

.curl-hint code {
  background: #f1f5f9;
  padding: 2px 6px;
  border-radius: 4px;
}
</style>
