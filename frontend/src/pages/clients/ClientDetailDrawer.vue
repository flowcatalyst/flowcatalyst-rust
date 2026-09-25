<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useConfirm } from "primevue/useconfirm";
import { clientsApi, type Client, type ClientApplication } from "@/api/clients";
import { getErrorMessage } from "@/utils/errors";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";
import { useDirtyForm } from "@/composables/useDirtyForm";

const emit = defineEmits<{
	changed: [];
}>();

const route = useRoute();
const router = useRouter();
const confirm = useConfirm();

const editing = ref(false);

// Edit form
const editName = ref("");

const { dirty, markClean, reset: resetDirty } = useDirtyForm(() => ({
	name: editName.value,
}));

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { id, goToList } = useDrawerRoute({
	listPath: "/clients",
	dirty: computed(() => editing.value && dirty.value),
});

const loading = ref(true);
const loadError = ref<string | null>(null);
const client = ref<Client | null>(null);
const saving = ref(false);

// Applications picker: [available, enabled]
const applications = ref<[ClientApplication[], ClientApplication[]]>([[], []]);
const loadingApps = ref(false);
const savingApps = ref(false);

const availableApps = computed(() => applications.value[0]);
const enabledApps = computed(() => applications.value[1]);

// Reactive param: the drawer instance is reused when switching between rows.
watch(
	id,
	async (value) => {
		if (!value) return;
		editing.value = false;
		resetDirty();
		applications.value = [[], []];
		await Promise.all([loadClient(value), loadApplications(value)]);
		if (route.query["edit"] === "true") {
			startEditing();
		}
	},
	{ immediate: true },
);

async function loadClient(clientId: string) {
	loading.value = true;
	loadError.value = null;
	try {
		client.value = await clientsApi.get(clientId);
	} catch {
		client.value = null;
		loadError.value = "Client not found";
	} finally {
		loading.value = false;
	}
}

async function loadApplications(clientId: string) {
	loadingApps.value = true;
	try {
		const response = await clientsApi.getApplications(clientId);
		const available: ClientApplication[] = [];
		const enabled: ClientApplication[] = [];

		for (const app of response.applications) {
			if (app.enabledForClient) {
				enabled.push(app);
			} else {
				available.push(app);
			}
		}

		applications.value = [available, enabled];
	} catch (error) {
		console.error("Failed to load applications:", error);
	} finally {
		loadingApps.value = false;
	}
}

async function saveApplications() {
	if (!client.value) return;

	savingApps.value = true;
	try {
		const enabledIds = applications.value[1].map((app) => app.id);
		await clientsApi.updateApplications(client.value.id, enabledIds);
		toast.success("Success", "Applications updated");
		emit("changed");
	} catch {
	} finally {
		savingApps.value = false;
	}
}

// Login branding is a full page, not a drawer — navigate away.
function viewLoginBranding() {
	if (!client.value) return;
	void router.push(`/clients/${client.value.id}/theme`);
}

function startEditing() {
	if (client.value) {
		editName.value = client.value.name;
		editing.value = true;
		markClean();
	}
}

function cancelEditing() {
	editing.value = false;
	resetDirty();
}

async function saveChanges() {
	if (!client.value) return;

	saving.value = true;
	const clientId = client.value.id;
	try {
		await clientsApi.update(clientId, {
			name: editName.value,
		});
		await loadClient(clientId);
		editing.value = false;
		resetDirty();
		toast.success("Success", "Client updated");
		emit("changed");
	} catch {
	} finally {
		saving.value = false;
	}
}

function confirmActivate() {
	confirm.require({
		message: "Activate this client?",
		header: "Activate Client",
		icon: "pi pi-check-circle",
		acceptLabel: "Activate",
		accept: activateClient,
	});
}

async function activateClient() {
	if (!client.value) return;
	try {
		await clientsApi.activate(client.value.id);
		client.value = await clientsApi.get(client.value.id);
		toast.success("Success", "Client activated");
		emit("changed");
	} catch {
	}
}

function confirmSuspend() {
	confirm.require({
		message: "Suspend this client? Users will not be able to access it.",
		header: "Suspend Client",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Suspend",
		acceptClass: "p-button-warning",
		accept: () => suspendClient("Manual suspension"),
	});
}

async function suspendClient(reason: string) {
	if (!client.value) return;
	try {
		await clientsApi.suspend(client.value.id, reason);
		client.value = await clientsApi.get(client.value.id);
		toast.success("Success", "Client suspended");
		emit("changed");
	} catch {
	}
}

function confirmDeactivate() {
	confirm.require({
		message: "Deactivate this client? This is a soft delete.",
		header: "Deactivate Client",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Deactivate",
		acceptClass: "p-button-danger",
		accept: () => deactivateClient("Manual deactivation"),
	});
}

async function deactivateClient(reason: string) {
	if (!client.value) return;
	try {
		await clientsApi.deactivate(client.value.id, reason);
		toast.success("Success", "Client deactivated");
		emit("changed");
		// The client is now soft-deleted — re-fetching its detail would 404, so
		// close back to the listing rather than staying on a dead detail view.
		editing.value = false;
		void drawer.value?.close(true);
	} catch (e) {
		toast.error("Error", getErrorMessage(e, "Failed to deactivate client"));
	}
}

function getStatusSeverity(status: string) {
	switch (status) {
		case "ACTIVE":
			return "success";
		case "SUSPENDED":
			return "warn";
		case "INACTIVE":
			return "secondary";
		default:
			return "secondary";
	}
}

function formatDate(dateString: string) {
	return new Date(dateString).toLocaleString();
}

function copyClientId() {
	if (!client.value) return;
	void navigator.clipboard.writeText(client.value.id);
	toast.info("Copied", "Client ID copied to clipboard");
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    size="wide"
    :title="client?.name || 'Client'"
    :subtitle="client?.identifier"
    :loading="loading"
    :error="loadError"
    :dirty="editing && dirty"
    @close="goToList()"
  >
    <template v-if="client" #header-extra>
      <Tag :value="client.status" :severity="getStatusSeverity(client.status)" />
    </template>

    <template v-if="client">
      <!-- Details -->
      <FcFormSection title="Client Details" flat>
        <template v-if="!editing" #actions>
          <Button icon="pi pi-pencil" label="Edit" text @click="startEditing" />
        </template>

        <template v-if="editing">
          <div class="fc-form-grid">
            <FcFormField label="Name" span>
              <template #default="{ id: fieldId }">
                <InputText :id="fieldId" v-model="editName" />
              </template>
            </FcFormField>
          </div>
        </template>

        <template v-else>
          <div class="fc-detail-grid">
            <FcDetailField label="Client ID">
              <span class="client-id-row">
                <code>{{ client.id }}</code>
                <Button
                  icon="pi pi-copy"
                  text
                  rounded
                  size="small"
                  title="Copy client ID"
                  @click="copyClientId"
                />
              </span>
            </FcDetailField>
            <FcDetailField label="Identifier">
              <code>{{ client.identifier }}</code>
            </FcDetailField>
            <FcDetailField label="Name" :value="client.name" />
            <FcDetailField label="Status">
              <Tag :value="client.status" :severity="getStatusSeverity(client.status)" />
            </FcDetailField>
            <FcDetailField
              v-if="client.statusReason"
              label="Status Reason"
              :value="client.statusReason"
            />
            <FcDetailField label="Created" :value="formatDate(client.createdAt)" />
            <FcDetailField label="Updated" :value="formatDate(client.updatedAt)" />
          </div>
        </template>
      </FcFormSection>

      <!-- Applications -->
      <FcFormSection title="Applications" flat>
        <template #actions>
          <Button
            label="Save"
            icon="pi pi-save"
            :loading="savingApps"
            :disabled="savingApps"
            @click="saveApplications"
          />
        </template>

        <div v-if="loadingApps" class="loading-apps">
          <ProgressSpinner strokeWidth="3" style="width: 30px; height: 30px" />
          <span>Loading applications...</span>
        </div>
        <PickList
          v-else
          v-model="applications"
          dataKey="id"
          breakpoint="960px"
          :pt="{
            list: { style: { height: '300px' } },
          }"
        >
          <template #sourceheader>
            <span class="picklist-header">Available ({{ availableApps.length }})</span>
          </template>
          <template #targetheader>
            <span class="picklist-header">Enabled ({{ enabledApps.length }})</span>
          </template>
          <template #item="{ item }">
            <div class="app-item">
              <div class="app-info">
                <span class="app-name">{{ item.name }}</span>
                <span class="app-code">{{ item.code }}</span>
              </div>
              <Tag v-if="!item.active" value="Inactive" severity="secondary" class="app-status" />
            </div>
          </template>
        </PickList>
        <p class="help-text">
          Move applications between lists to enable or disable them for this client. Click Save to
          apply changes.
        </p>
      </FcFormSection>

      <!-- Login Branding -->
      <FcFormSection title="Login Branding" flat>
        <div class="action-items">
          <div class="action-item">
            <div class="action-info">
              <strong>Login Branding</strong>
              <p>Customize the sign-in, forgot-password and reset-password pages for this client.</p>
            </div>
            <Button label="Edit Branding" icon="pi pi-palette" outlined @click="viewLoginBranding" />
          </div>
        </div>
      </FcFormSection>

      <!-- Actions -->
      <FcFormSection v-if="!editing" title="Actions" flat>
        <div class="action-items">
          <div v-if="client.status !== 'ACTIVE'" class="action-item">
            <div class="action-info">
              <strong>Activate Client</strong>
              <p>Make this client active and accessible.</p>
            </div>
            <Button label="Activate" severity="success" outlined @click="confirmActivate" />
          </div>

          <div v-if="client.status === 'ACTIVE'" class="action-item">
            <div class="action-info">
              <strong>Suspend Client</strong>
              <p>Temporarily disable access to this client.</p>
            </div>
            <Button label="Suspend" severity="warn" outlined @click="confirmSuspend" />
          </div>

          <div v-if="client.status !== 'INACTIVE'" class="action-item">
            <div class="action-info">
              <strong>Deactivate Client</strong>
              <p>Soft delete this client. Can be reactivated later.</p>
            </div>
            <Button label="Deactivate" severity="danger" outlined @click="confirmDeactivate" />
          </div>
        </div>
      </FcFormSection>
    </template>

    <template v-if="editing" #footer>
      <FcFormActions :bordered="false">
        <Button v-if="dirty" label="Discard" severity="secondary" outlined @click="cancelEditing" />
        <Button label="Save" :disabled="!dirty" :loading="saving" @click="saveChanges" />
      </FcFormActions>
    </template>
  </EntityDrawer>
</template>

<style scoped>
.client-id-row {
	display: inline-flex;
	align-items: center;
	gap: 0.25rem;
}

.loading-apps {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding: 40px;
  color: #64748b;
}

.picklist-header {
  font-weight: 600;
  font-size: 14px;
}

.app-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 0;
}

.app-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.app-name {
  font-weight: 500;
}

.app-code {
  font-size: 12px;
  color: #64748b;
  font-family: monospace;
}

.app-status {
  font-size: 11px;
}

.help-text {
  margin-top: 12px;
  font-size: 13px;
  color: #64748b;
}

.help-text code {
  font-size: 12px;
  word-break: break-all;
}

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

:deep(.p-picklist) {
  max-width: 100%;
}
</style>
