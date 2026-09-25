<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useConfirm } from "primevue/useconfirm";
import {
	eventTypesApi,
	type EventType,
	type SpecVersion,
} from "@/api/event-types";
import SchemaViewerDialog from "./SchemaViewerDialog.vue";
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

const loading = ref(true);
const loadError = ref<string | null>(null);
const eventType = ref<EventType | null>(null);
const saving = ref(false);

// Schema viewer
const viewerVisible = ref(false);
const viewerSpecVersion = ref<SpecVersion | null>(null);

function viewSchema(sv: SpecVersion) {
	viewerSpecVersion.value = sv;
	viewerVisible.value = true;
}

// Edit form
const editName = ref("");
const editDescription = ref("");

const { dirty, markClean, reset: resetDirty } = useDirtyForm(() => ({
	name: editName.value,
	description: editDescription.value,
}));

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { id, goToList } = useDrawerRoute({
	listPath: "/event-types",
	dirty: computed(() => editing.value && dirty.value),
});

const canArchive = computed(() => {
	const et = eventType.value;
	if (!et || et.status !== "CURRENT") return false;
	if (et.specVersions.length === 0) return true;
	return et.specVersions.every((sv) => sv.status === "DEPRECATED");
});

const canDelete = computed(() => {
	const et = eventType.value;
	if (!et) return false;
	if (et.status === "ARCHIVED") return true;
	if (et.status === "CURRENT" && et.specVersions.length === 0) return true;
	return (
		et.status === "CURRENT" &&
		et.specVersions.every((sv) => sv.status === "FINALISING")
	);
});

// Reactive param: the drawer instance is reused when switching between rows.
watch(
	id,
	async (value) => {
		if (!value) return;
		editing.value = false;
		resetDirty();
		viewerVisible.value = false;
		viewerSpecVersion.value = null;
		await loadEventType(value);
		if (route.query["edit"] === "true") {
			startEditing();
		}
	},
	{ immediate: true },
);

async function loadEventType(eventTypeId: string) {
	loading.value = true;
	loadError.value = null;
	try {
		eventType.value = await eventTypesApi.get(eventTypeId);
	} catch {
		eventType.value = null;
		loadError.value = "Event type not found";
	} finally {
		loading.value = false;
	}
}

function startEditing() {
	if (eventType.value) {
		editName.value = eventType.value.name;
		editDescription.value = eventType.value.description || "";
		editing.value = true;
		markClean();
	}
}

function cancelEditing() {
	editing.value = false;
	resetDirty();
}

async function saveChanges() {
	if (!eventType.value) return;

	saving.value = true;
	const eventTypeId = eventType.value.id;
	try {
		await eventTypesApi.update(eventTypeId, {
			name: editName.value,
			description: editDescription.value,
		});
		await loadEventType(eventTypeId);
		editing.value = false;
		resetDirty();
		toast.success("Success", "Event type updated");
		emit("changed");
	} catch {
	} finally {
		saving.value = false;
	}
}

// Add-schema is a full-page carve-out; leaving the drawer is a normal
// route-leave (the dirty guard prompts if an edit is in progress).
function goToAddSchema() {
	if (!id.value) return;
	void router.push(`/event-types/${id.value}/add-schema`);
}

function getSchemaStatusSeverity(status: string) {
	switch (status) {
		case "CURRENT":
			return "success";
		case "FINALISING":
			return "info";
		case "DEPRECATED":
			return "warn";
		default:
			return "secondary";
	}
}

function formatSchemaType(type: string) {
	switch (type) {
		case "JSON_SCHEMA":
			return "JSON Schema";
		case "PROTO":
			return "Protocol Buffers";
		case "XSD":
			return "XML Schema";
		default:
			return type;
	}
}

function confirmFinalise(sv: SpecVersion) {
	confirm.require({
		message: `Finalise schema version ${sv.version}? This makes it the current version.`,
		header: "Finalise Schema",
		icon: "pi pi-check-circle",
		acceptLabel: "Finalise",
		accept: () => finaliseSchema(sv.version),
	});
}

async function finaliseSchema(version: string) {
	if (!eventType.value) return;
	try {
		eventType.value = await eventTypesApi.finaliseSchema(
			eventType.value.id,
			version,
		);
		toast.success("Success", `Schema ${version} finalised`);
		emit("changed");
	} catch {
	}
}

function confirmDeprecate(sv: SpecVersion) {
	confirm.require({
		message: `Deprecate schema version ${sv.version}?`,
		header: "Deprecate Schema",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Deprecate",
		acceptClass: "p-button-warning",
		accept: () => deprecateSchema(sv.version),
	});
}

async function deprecateSchema(version: string) {
	if (!eventType.value) return;
	try {
		eventType.value = await eventTypesApi.deprecateSchema(
			eventType.value.id,
			version,
		);
		toast.success("Success", `Schema ${version} deprecated`);
		emit("changed");
	} catch {
	}
}

function confirmArchive() {
	confirm.require({
		message:
			"Archive this event type? No new events can be created for archived types.",
		header: "Archive Event Type",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Archive",
		acceptClass: "p-button-warning",
		accept: archiveEventType,
	});
}

async function archiveEventType() {
	if (!eventType.value) return;
	try {
		eventType.value = await eventTypesApi.archive(eventType.value.id);
		toast.success("Success", "Event type archived");
		emit("changed");
	} catch {
	}
}

function confirmDelete() {
	confirm.require({
		message: "Delete this event type? This cannot be undone.",
		header: "Delete Event Type",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Delete",
		acceptClass: "p-button-danger",
		accept: deleteEventType,
	});
}

async function deleteEventType() {
	if (!eventType.value) return;
	try {
		await eventTypesApi.delete(eventType.value.id);
		toast.success("Success", "Event type deleted");
		emit("changed");
		editing.value = false;
		void drawer.value?.close(true);
	} catch {
	}
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    :title="eventType?.name || 'Event Type'"
    :subtitle="eventType?.code"
    size="wide"
    :loading="loading"
    :error="loadError"
    :dirty="editing && dirty"
    @close="goToList()"
  >
    <template v-if="eventType" #header-extra>
      <Tag
        :value="eventType.status"
        :severity="eventType.status === 'CURRENT' ? 'success' : 'secondary'"
      />
    </template>

    <template v-if="eventType">
      <!-- Details -->
      <FcFormSection title="Event Type Details" flat>
        <template v-if="!editing && eventType.status !== 'ARCHIVED'" #actions>
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
          </div>
        </template>

        <template v-else>
          <div class="fc-detail-grid">
            <FcDetailField label="Code" span>
              <div class="code-display">
                <span class="code-segment app">{{ eventType.application }}</span>
                <span class="code-separator">:</span>
                <span class="code-segment subdomain">{{ eventType.subdomain }}</span>
                <span class="code-separator">:</span>
                <span class="code-segment aggregate">{{ eventType.aggregate }}</span>
                <span class="code-separator">:</span>
                <span class="code-segment event">{{ eventType.event }}</span>
              </div>
            </FcDetailField>
            <FcDetailField label="Name" :value="eventType.name" />
            <FcDetailField label="Description" :value="eventType.description" />
            <FcDetailField label="Client Scoped">
              <Tag
                :value="eventType.clientScoped ? 'Yes' : 'No'"
                :severity="eventType.clientScoped ? 'info' : 'secondary'"
              />
            </FcDetailField>
          </div>
        </template>
      </FcFormSection>

      <!-- Schema Versions -->
      <FcFormSection title="Schema Versions" flat>
        <template v-if="eventType.status === 'CURRENT'" #actions>
          <Button icon="pi pi-plus" label="Add Schema" text @click="goToAddSchema" />
        </template>

        <div v-if="eventType.specVersions.length === 0" class="empty-state">
          <i class="pi pi-file"></i>
          <p>No schema versions defined yet.</p>
          <Button
            v-if="eventType.status === 'CURRENT'"
            label="Add First Schema"
            icon="pi pi-plus"
            @click="goToAddSchema"
          />
        </div>

        <DataTable v-else :value="eventType.specVersions" size="small">
          <Column header="Version" style="width: 15%">
            <template #body="{ data }">
              <span class="version-text">{{ data.version }}</span>
            </template>
          </Column>
          <Column header="MIME Type" style="width: 20%">
            <template #body="{ data }">
              <code class="mime-type">{{ data.mimeType }}</code>
            </template>
          </Column>
          <Column header="Schema Type" style="width: 20%">
            <template #body="{ data }">
              {{ formatSchemaType(data.schemaType) }}
            </template>
          </Column>
          <Column header="Status" style="width: 15%">
            <template #body="{ data }">
              <Tag :value="data.status" :severity="getSchemaStatusSeverity(data.status)" />
            </template>
          </Column>
          <Column header="Actions" style="width: 30%">
            <template #body="{ data }">
              <div class="action-buttons">
                <Button
                  v-if="data.schema"
                  icon="pi pi-eye"
                  rounded
                  text
                  v-tooltip="'View Schema'"
                  @click="viewSchema(data)"
                />
                <Button
                  v-if="data.status === 'FINALISING'"
                  icon="pi pi-check"
                  rounded
                  text
                  severity="success"
                  v-tooltip="'Finalise'"
                  @click="confirmFinalise(data)"
                />
                <Button
                  v-if="data.status === 'CURRENT'"
                  icon="pi pi-ban"
                  rounded
                  text
                  severity="warn"
                  v-tooltip="'Deprecate'"
                  @click="confirmDeprecate(data)"
                />
              </div>
            </template>
          </Column>
        </DataTable>
      </FcFormSection>

      <!-- Danger Zone -->
      <FcFormSection title="Danger Zone" flat class="danger-zone">
        <div class="danger-actions">
          <div v-if="eventType.status === 'CURRENT'" class="danger-item">
            <div class="danger-info">
              <strong>Archive Event Type</strong>
              <p>Requires all schemas to be deprecated first.</p>
            </div>
            <Button
              label="Archive"
              severity="warn"
              outlined
              :disabled="!canArchive"
              @click="confirmArchive"
            />
          </div>

          <div class="danger-item">
            <div class="danger-info">
              <strong>Delete Event Type</strong>
              <p>Permanently delete this event type.</p>
            </div>
            <Button
              label="Delete"
              severity="danger"
              outlined
              :disabled="!canDelete"
              @click="confirmDelete"
            />
          </div>
        </div>
      </FcFormSection>

      <!-- Schema Viewer Dialog -->
      <SchemaViewerDialog
        v-model:visible="viewerVisible"
        :specVersion="viewerSpecVersion"
        :eventCode="eventType.code"
      />
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
.empty-state {
  text-align: center;
  padding: 48px;
  color: #64748b;
}

.empty-state i {
  font-size: 48px;
  color: #cbd5e1;
  margin-bottom: 16px;
}

.version-text {
  font-family: monospace;
  font-weight: 500;
}

.mime-type {
  font-size: 13px;
  background: #f1f5f9;
  padding: 2px 6px;
  border-radius: 4px;
}

.action-buttons {
  display: flex;
  gap: 4px;
}

.danger-zone :deep(.section-title) {
  color: #dc2626;
}

.danger-actions {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.danger-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  padding: 16px;
  background: #fafafa;
  border-radius: 8px;
  border: 1px solid #e5e7eb;
}

.danger-info strong {
  display: block;
  margin-bottom: 4px;
}

.danger-info p {
  margin: 0;
  font-size: 13px;
  color: #64748b;
}
</style>
