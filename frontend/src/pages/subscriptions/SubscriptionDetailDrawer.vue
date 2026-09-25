<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { computed, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { useConfirm } from "primevue/useconfirm";
import {
	subscriptionsApi,
	type Subscription,
	type SubscriptionMode,
} from "@/api/subscriptions";
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
const editEndpoint = ref("");
const editConnectionId = ref("");
const editQueue = ref("");
const editMaxAgeSeconds = ref<number | null>(null);
const editDelaySeconds = ref<number | null>(null);
const editSequence = ref<number | null>(null);
const editTimeoutSeconds = ref<number | null>(null);
const editMode = ref<SubscriptionMode>("NEXT_ON_ERROR"); // platform default; replaced on load

const { dirty, markClean, reset: resetDirty } = useDirtyForm(() => ({
	name: editName.value,
	description: editDescription.value,
	endpoint: editEndpoint.value,
	connectionId: editConnectionId.value,
	queue: editQueue.value,
	maxAgeSeconds: editMaxAgeSeconds.value,
	delaySeconds: editDelaySeconds.value,
	sequence: editSequence.value,
	timeoutSeconds: editTimeoutSeconds.value,
	mode: editMode.value,
}));

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { id, goToList } = useDrawerRoute({
	listPath: "/subscriptions",
	dirty: computed(() => editing.value && dirty.value),
});

const loading = ref(true);
const loadError = ref<string | null>(null);
const subscription = ref<Subscription | null>(null);
const saving = ref(false);

const modeOptions = [
	{ label: "Immediate", value: "IMMEDIATE" },
	{ label: "Next on Error", value: "NEXT_ON_ERROR" },
	{ label: "Block on Error", value: "BLOCK_ON_ERROR" },
];

// Reactive param: the drawer instance is reused when switching between rows.
watch(
	id,
	async (value) => {
		if (!value) return;
		editing.value = false;
		resetDirty();
		await loadSubscription(value);
		if (route.query["edit"] === "true") {
			startEditing();
		}
	},
	{ immediate: true },
);

async function loadSubscription(subscriptionId: string) {
	loading.value = true;
	loadError.value = null;
	try {
		subscription.value = await subscriptionsApi.get(subscriptionId);
	} catch {
		subscription.value = null;
		loadError.value = "Subscription not found";
	} finally {
		loading.value = false;
	}
}

function startEditing() {
	if (subscription.value) {
		editName.value = subscription.value.name;
		editDescription.value = subscription.value.description || "";
		editEndpoint.value = subscription.value.endpoint || "";
		editConnectionId.value = subscription.value.connectionId || "";
		editQueue.value = subscription.value.queue || "";
		editMaxAgeSeconds.value = subscription.value.maxAgeSeconds;
		editDelaySeconds.value = subscription.value.delaySeconds;
		editSequence.value = subscription.value.sequence;
		editTimeoutSeconds.value = subscription.value.timeoutSeconds;
		// Wire mode is plain string; the form narrows to the known wire values.
		editMode.value = subscription.value.mode as SubscriptionMode;
		editing.value = true;
		markClean();
	}
}

function cancelEditing() {
	editing.value = false;
	resetDirty();
}

async function saveChanges() {
	if (!subscription.value) return;

	saving.value = true;
	const subscriptionId = subscription.value.id;
	try {
		await subscriptionsApi.update(subscriptionId, {
			name: editName.value,
			description: editDescription.value || undefined,
			endpoint: editEndpoint.value,
			connectionId: editConnectionId.value,
			queue: editQueue.value,
			maxAgeSeconds: editMaxAgeSeconds.value || undefined,
			delaySeconds: editDelaySeconds.value || undefined,
			sequence: editSequence.value || undefined,
			timeoutSeconds: editTimeoutSeconds.value || undefined,
			mode: editMode.value,
		});
		await loadSubscription(subscriptionId);
		editing.value = false;
		resetDirty();
		toast.success("Success", "Subscription updated");
		emit("changed");
	} catch {
		// update errors surface via the global error toast
	} finally {
		saving.value = false;
	}
}

function confirmPause() {
	confirm.require({
		message: "Pause this subscription? It will stop creating dispatch jobs.",
		header: "Pause Subscription",
		icon: "pi pi-pause",
		acceptLabel: "Pause",
		acceptClass: "p-button-warning",
		accept: pauseSubscription,
	});
}

async function pauseSubscription() {
	if (!subscription.value) return;
	try {
		await subscriptionsApi.pause(subscription.value.id);
		subscription.value = await subscriptionsApi.get(subscription.value.id);
		toast.success("Success", "Subscription paused");
		emit("changed");
	} catch {
		// errors surface via the global error toast
	}
}

function confirmResume() {
	confirm.require({
		message: "Resume this subscription?",
		header: "Resume Subscription",
		icon: "pi pi-play",
		acceptLabel: "Resume",
		accept: resumeSubscription,
	});
}

async function resumeSubscription() {
	if (!subscription.value) return;
	try {
		await subscriptionsApi.resume(subscription.value.id);
		subscription.value = await subscriptionsApi.get(subscription.value.id);
		toast.success("Success", "Subscription resumed");
		emit("changed");
	} catch {
		// errors surface via the global error toast
	}
}

function confirmDelete() {
	confirm.require({
		message: "Delete this subscription? This action cannot be undone.",
		header: "Delete Subscription",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Delete",
		acceptClass: "p-button-danger",
		accept: deleteSubscription,
	});
}

async function deleteSubscription() {
	if (!subscription.value) return;
	try {
		await subscriptionsApi.delete(subscription.value.id);
		toast.success("Success", "Subscription deleted");
		emit("changed");
		editing.value = false;
		void drawer.value?.close(true);
	} catch {
		// errors surface via the global error toast
	}
}

// Wire status/mode are plain strings (spec carries no enum); default covers unknowns.
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

function getModeLabel(mode: string) {
	switch (mode) {
		case "IMMEDIATE":
			return "Immediate";
		case "NEXT_ON_ERROR":
			return "Next on Error";
		case "BLOCK_ON_ERROR":
			return "Block on Error";
		default:
			return mode;
	}
}

function formatDate(dateString: string) {
	return new Date(dateString).toLocaleString();
}

function getScopeLabel(sub: Subscription) {
	if (sub.clientIdentifier) {
		return sub.clientIdentifier;
	}
	return "Anchor-level (no client)";
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    :title="subscription?.name || 'Subscription'"
    :subtitle="subscription?.code"
    size="wide"
    :loading="loading"
    :error="loadError"
    :dirty="editing && dirty"
    @close="goToList()"
  >
    <template v-if="subscription" #header-extra>
      <Tag :value="subscription.status" :severity="getStatusSeverity(subscription.status)" />
    </template>

    <template v-if="subscription">
      <!-- Details -->
      <FcFormSection title="Subscription Details" flat>
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
            <FcFormField label="Endpoint URL" span>
              <template #default="{ id: fieldId }">
                <InputText :id="fieldId" v-model="editEndpoint" />
              </template>
            </FcFormField>
            <FcFormField label="Connection ID">
              <template #default="{ id: fieldId }">
                <InputText :id="fieldId" v-model="editConnectionId" />
              </template>
            </FcFormField>
            <FcFormField label="Queue">
              <template #default="{ id: fieldId }">
                <InputText :id="fieldId" v-model="editQueue" />
              </template>
            </FcFormField>
            <FcFormField label="Max Age (seconds)">
              <template #default="{ id: fieldId }">
                <InputNumber :inputId="fieldId" v-model="editMaxAgeSeconds" :min="1" />
              </template>
            </FcFormField>
            <FcFormField label="Timeout (seconds)">
              <template #default="{ id: fieldId }">
                <InputNumber :inputId="fieldId" v-model="editTimeoutSeconds" :min="1" />
              </template>
            </FcFormField>
            <FcFormField label="Delay (seconds)">
              <template #default="{ id: fieldId }">
                <InputNumber :inputId="fieldId" v-model="editDelaySeconds" :min="0" />
              </template>
            </FcFormField>
            <FcFormField label="Sequence">
              <template #default="{ id: fieldId }">
                <InputNumber :inputId="fieldId" v-model="editSequence" :min="1" />
              </template>
            </FcFormField>
            <FcFormField label="Mode">
              <template #default="{ id: fieldId }">
                <Select
                  :id="fieldId"
                  v-model="editMode"
                  :options="modeOptions"
                  optionLabel="label"
                  optionValue="value"
                />
              </template>
            </FcFormField>
          </div>
        </template>

        <template v-else>
          <div class="fc-detail-grid">
            <FcDetailField label="Code">
              <code>{{ subscription.code }}</code>
            </FcDetailField>
            <FcDetailField label="Name" :value="subscription.name" />
            <FcDetailField
              v-if="subscription.description"
              label="Description"
              :value="subscription.description"
              span
            />
            <FcDetailField label="Client Scope" :value="getScopeLabel(subscription)" />
            <FcDetailField label="Source" :value="subscription.source" />
            <FcDetailField label="Endpoint" span>
              <code class="endpoint-url">{{ subscription.endpoint }}</code>
            </FcDetailField>
            <FcDetailField v-if="subscription.connectionId" label="Connection" span>
              <code>{{ subscription.connectionId }}</code>
            </FcDetailField>
            <FcDetailField label="Queue">
              <code>{{ subscription.queue }}</code>
            </FcDetailField>
            <FcDetailField label="Dispatch Pool">
              <code>{{ subscription.dispatchPoolCode }}</code>
            </FcDetailField>
            <FcDetailField label="Mode" :value="getModeLabel(subscription.mode)" />
            <FcDetailField label="Max Age" :value="`${subscription.maxAgeSeconds} seconds`" />
            <FcDetailField label="Delay" :value="`${subscription.delaySeconds} seconds`" />
            <FcDetailField label="Timeout" :value="`${subscription.timeoutSeconds} seconds`" />
            <FcDetailField label="Sequence" :value="subscription.sequence" />
            <FcDetailField label="Status">
              <Tag
                :value="subscription.status"
                :severity="getStatusSeverity(subscription.status)"
              />
            </FcDetailField>
            <FcDetailField label="Created" :value="formatDate(subscription.createdAt)" />
            <FcDetailField label="Updated" :value="formatDate(subscription.updatedAt)" />
          </div>
        </template>
      </FcFormSection>

      <!-- Event Types -->
      <FcFormSection :title="`Event Types (${subscription.eventTypes?.length || 0})`" flat>
        <DataTable :value="subscription.eventTypes" stripedRows>
          <template #empty>No event types configured</template>
          <Column field="eventTypeCode" header="Event Type Code" />
          <Column field="specVersion" header="Spec Version" />
        </DataTable>
      </FcFormSection>

      <!-- Actions -->
      <FcFormSection v-if="!editing" title="Actions" flat>
        <div class="action-items">
          <div v-if="subscription.status === 'ACTIVE'" class="action-item">
            <div class="action-info">
              <strong>Pause Subscription</strong>
              <p>Stop creating dispatch jobs for this subscription.</p>
            </div>
            <Button
              label="Pause"
              icon="pi pi-pause"
              severity="warn"
              outlined
              @click="confirmPause"
            />
          </div>

          <div v-if="subscription.status === 'PAUSED'" class="action-item">
            <div class="action-info">
              <strong>Resume Subscription</strong>
              <p>Re-enable dispatch job creation.</p>
            </div>
            <Button
              label="Resume"
              icon="pi pi-play"
              severity="success"
              outlined
              @click="confirmResume"
            />
          </div>

          <div class="action-item">
            <div class="action-info">
              <strong>Delete Subscription</strong>
              <p>Permanently delete this subscription. Cannot be undone.</p>
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
.endpoint-url {
  font-size: 13px;
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
</style>
