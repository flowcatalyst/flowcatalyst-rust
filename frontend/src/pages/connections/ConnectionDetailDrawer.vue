<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { computed, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { useConfirm } from "primevue/useconfirm";
import { connectionsApi, type Connection } from "@/api/connections";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";
import { useDirtyForm } from "@/composables/useDirtyForm";

const emit = defineEmits<{
	changed: [];
}>();

const route = useRoute();
const confirm = useConfirm();

const editing = ref(false);

// Edit form
const editName = ref("");
const editDescription = ref("");
const editExternalId = ref("");

const { dirty, markClean, reset: resetDirty } = useDirtyForm(() => ({
	name: editName.value,
	description: editDescription.value,
	externalId: editExternalId.value,
}));

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { id, goToList } = useDrawerRoute({
	listPath: "/connections",
	dirty: computed(() => editing.value && dirty.value),
});

const loading = ref(true);
const loadError = ref<string | null>(null);
const connection = ref<Connection | null>(null);
const saving = ref(false);

// Reactive param: the drawer instance is reused when switching between rows.
watch(
	id,
	async (value) => {
		if (!value) return;
		editing.value = false;
		resetDirty();
		await loadConnection(value);
		if (route.query["edit"] === "true") {
			startEditing();
		}
	},
	{ immediate: true },
);

async function loadConnection(connectionId: string) {
	loading.value = true;
	loadError.value = null;
	try {
		connection.value = await connectionsApi.get(connectionId);
	} catch {
		connection.value = null;
		loadError.value = "Connection not found";
	} finally {
		loading.value = false;
	}
}

function startEditing() {
	if (connection.value) {
		editName.value = connection.value.name;
		editDescription.value = connection.value.description || "";
		editExternalId.value = connection.value.externalId || "";
		editing.value = true;
		markClean();
	}
}

function cancelEditing() {
	editing.value = false;
	resetDirty();
}

async function saveChanges() {
	if (!connection.value) return;

	saving.value = true;
	const connectionId = connection.value.id;
	try {
		await connectionsApi.update(connectionId, {
			name: editName.value,
			description: editDescription.value || undefined,
			externalId: editExternalId.value || undefined,
		});
		await loadConnection(connectionId);
		editing.value = false;
		resetDirty();
		toast.success("Success", "Connection updated");
		emit("changed");
	} catch {
		// update errors surface via the global error toast
	} finally {
		saving.value = false;
	}
}

function confirmActivate() {
	confirm.require({
		message: "Activate this connection?",
		header: "Activate Connection",
		icon: "pi pi-check-circle",
		acceptLabel: "Activate",
		accept: activateConnection,
	});
}

async function activateConnection() {
	if (!connection.value) return;
	try {
		await connectionsApi.activate(connection.value.id);
		connection.value = await connectionsApi.get(connection.value.id);
		toast.success("Success", "Connection activated");
		emit("changed");
	} catch {
		// errors surface via the global error toast
	}
}

function confirmPause() {
	confirm.require({
		message: "Pause this connection? Subscriptions using it will stop dispatching.",
		header: "Pause Connection",
		icon: "pi pi-pause",
		acceptLabel: "Pause",
		acceptClass: "p-button-warning",
		accept: pauseConnection,
	});
}

async function pauseConnection() {
	if (!connection.value) return;
	try {
		await connectionsApi.pause(connection.value.id);
		connection.value = await connectionsApi.get(connection.value.id);
		toast.success("Success", "Connection paused");
		emit("changed");
	} catch {
		// errors surface via the global error toast
	}
}

function confirmDelete() {
	confirm.require({
		message: "Delete this connection? This action cannot be undone.",
		header: "Delete Connection",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Delete",
		acceptClass: "p-button-danger",
		accept: deleteConnection,
	});
}

async function deleteConnection() {
	if (!connection.value) return;
	try {
		await connectionsApi.delete(connection.value.id);
		toast.success("Success", "Connection deleted");
		emit("changed");
		editing.value = false;
		void drawer.value?.close(true);
	} catch {
		// errors surface via the global error toast
	}
}

// Takes the wire `status` (plain string in the generated type — the spec has
// no enums); the default branch covers anything outside ACTIVE/PAUSED.
function getStatusSeverity(status: string) {
	switch (status) {
		case "ACTIVE":
			return "success";
		case "PAUSED":
			return "warn";
		default:
			return "secondary";
	}
}

function formatDate(dateString: string) {
	return new Date(dateString).toLocaleString();
}

function getScopeLabel(conn: Connection) {
	if (conn.clientIdentifier) {
		return conn.clientIdentifier;
	}
	return "Anchor-level (no client)";
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    :title="connection?.name || 'Connection'"
    :subtitle="connection?.code"
    size="default"
    :loading="loading"
    :error="loadError"
    :dirty="editing && dirty"
    @close="goToList()"
  >
    <template v-if="connection" #header-extra>
      <Tag :value="connection.status" :severity="getStatusSeverity(connection.status)" />
    </template>

    <template v-if="connection">
      <!-- Details -->
      <FcFormSection title="Connection Details" flat>
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
            <FcFormField label="Description" span>
              <template #default="{ id: fieldId }">
                <Textarea :id="fieldId" v-model="editDescription" rows="3" />
              </template>
            </FcFormField>
            <FcFormField label="External ID" span>
              <template #default="{ id: fieldId }">
                <InputText :id="fieldId" v-model="editExternalId" />
              </template>
            </FcFormField>
          </div>
        </template>

        <template v-else>
          <div class="fc-detail-grid">
            <FcDetailField label="Code">
              <code>{{ connection.code }}</code>
            </FcDetailField>
            <FcDetailField label="Name" :value="connection.name" />
            <FcDetailField
              v-if="connection.description"
              label="Description"
              :value="connection.description"
              span
            />
            <FcDetailField v-if="connection.externalId" label="External ID">
              <code>{{ connection.externalId }}</code>
            </FcDetailField>
            <FcDetailField label="Service Account" :value="connection.serviceAccountId" />
            <FcDetailField label="Scope" :value="getScopeLabel(connection)" />
            <FcDetailField label="Status">
              <Tag
                :value="connection.status"
                :severity="getStatusSeverity(connection.status)"
              />
            </FcDetailField>
            <FcDetailField label="Created" :value="formatDate(connection.createdAt)" />
            <FcDetailField label="Updated" :value="formatDate(connection.updatedAt)" />
          </div>
        </template>
      </FcFormSection>

      <!-- Actions -->
      <FcFormSection v-if="!editing" title="Actions" flat>
        <div class="action-items">
          <div v-if="connection.status === 'PAUSED'" class="action-item">
            <div class="action-info">
              <strong>Activate Connection</strong>
              <p>Enable this connection for event delivery.</p>
            </div>
            <Button label="Activate" severity="success" outlined @click="confirmActivate" />
          </div>

          <div v-if="connection.status === 'ACTIVE'" class="action-item">
            <div class="action-info">
              <strong>Pause Connection</strong>
              <p>Temporarily stop event delivery through this connection.</p>
            </div>
            <Button
              label="Pause"
              icon="pi pi-pause"
              severity="warn"
              outlined
              @click="confirmPause"
            />
          </div>

          <div class="action-item">
            <div class="action-info">
              <strong>Delete Connection</strong>
              <p>Permanently delete this connection. Cannot be undone.</p>
            </div>
            <Button
              label="Delete"
              icon="pi pi-trash"
              severity="danger"
              outlined
              @click="confirmDelete"
            />
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
</style>
