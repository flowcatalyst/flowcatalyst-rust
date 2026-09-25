<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useConfirm } from "primevue/useconfirm";
import { toast } from "@/utils/errorBus";
import {
	scheduledJobsApi,
	type ScheduledJob,
	type ScheduledJobInstance,
} from "@/api/scheduled-jobs";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";
import { useDirtyForm } from "@/composables/useDirtyForm";

const emit = defineEmits<{
	changed: [];
}>();

const router = useRouter();
const confirm = useConfirm();

const editing = ref(false);
const saving = ref(false);

// Edit form — only the fields the platform allows updating (immutable
// fields like clientId/applicationId are excluded, matching the backend's
// UpdateScheduledJobRequest shape).
const editName = ref("");
const editDescription = ref("");
const editCrons = ref<string[]>([""]);
const editTimezone = ref("UTC");
const editPayloadJson = ref("");
const editConcurrent = ref(false);
const editTracksCompletion = ref(false);
const editTimeoutSeconds = ref<number | null>(null);
const editDeliveryMaxAttempts = ref<number | null>(3);
const editTargetUrl = ref("");

const { dirty, markClean, reset: resetDirty } = useDirtyForm(() => ({
	name: editName.value,
	description: editDescription.value,
	crons: editCrons.value,
	timezone: editTimezone.value,
	payloadJson: editPayloadJson.value,
	concurrent: editConcurrent.value,
	tracksCompletion: editTracksCompletion.value,
	timeoutSeconds: editTimeoutSeconds.value,
	deliveryMaxAttempts: editDeliveryMaxAttempts.value,
	targetUrl: editTargetUrl.value,
}));

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { id, goToList } = useDrawerRoute({
	listPath: "/scheduled-jobs",
	dirty: computed(() => editing.value && dirty.value),
});

const loading = ref(true);
const loadError = ref<string | null>(null);
const job = ref<ScheduledJob | null>(null);
const recentInstances = ref<ScheduledJobInstance[]>([]);
const acting = ref(false);

// Reactive param: the drawer instance is reused when switching between rows.
watch(
	id,
	async (value) => {
		if (!value) return;
		job.value = null;
		recentInstances.value = [];
		editing.value = false;
		resetDirty();
		await load(value);
	},
	{ immediate: true },
);

async function load(jobId: string) {
	loading.value = true;
	loadError.value = null;
	try {
		job.value = await scheduledJobsApi.get(jobId);
		const result = await scheduledJobsApi.listInstances(jobId, {
			page: 0,
			size: 10,
		});
		recentInstances.value = result.data;
	} catch {
		job.value = null;
		loadError.value = "Scheduled job not found";
	} finally {
		loading.value = false;
	}
}

function startEditing() {
	if (!job.value) return;
	editName.value = job.value.name;
	editDescription.value = job.value.description ?? "";
	editCrons.value = job.value.crons.length > 0 ? [...job.value.crons] : [""];
	editTimezone.value = job.value.timezone;
	editPayloadJson.value = job.value.payload
		? JSON.stringify(job.value.payload, null, 2)
		: "";
	editConcurrent.value = job.value.concurrent;
	editTracksCompletion.value = job.value.tracksCompletion;
	editTimeoutSeconds.value = job.value.timeoutSeconds ?? null;
	editDeliveryMaxAttempts.value = job.value.deliveryMaxAttempts;
	editTargetUrl.value = job.value.targetUrl ?? "";
	editing.value = true;
	markClean();
}

function cancelEditing() {
	editing.value = false;
	resetDirty();
}

function addEditCron() {
	editCrons.value.push("");
}
function removeEditCron(idx: number) {
	if (editCrons.value.length > 1) editCrons.value.splice(idx, 1);
}

async function saveChanges() {
	if (!job.value) return;
	if (!editName.value.trim()) {
		toast.warn("Missing fields", "Name is required");
		return;
	}
	const cleanCrons = editCrons.value.map((c) => c.trim()).filter(Boolean);
	if (cleanCrons.length === 0) {
		toast.warn("Missing cron", "At least one cron expression is required");
		return;
	}
	let parsedPayload: unknown = undefined;
	if (editPayloadJson.value.trim()) {
		try {
			parsedPayload = JSON.parse(editPayloadJson.value);
		} catch {
			toast.warn("Invalid payload", "Payload must be valid JSON or empty");
			return;
		}
	}

	const jobId = job.value.id;
	saving.value = true;
	try {
		await scheduledJobsApi.update(jobId, {
			name: editName.value.trim(),
			description: editDescription.value.trim() || undefined,
			crons: cleanCrons,
			timezone: editTimezone.value || undefined,
			payload: parsedPayload,
			concurrent: editConcurrent.value,
			tracksCompletion: editTracksCompletion.value,
			timeoutSeconds: editTimeoutSeconds.value ?? undefined,
			deliveryMaxAttempts: editDeliveryMaxAttempts.value ?? undefined,
			targetUrl: editTargetUrl.value.trim() || undefined,
		});
		await load(jobId);
		editing.value = false;
		resetDirty();
		toast.success("Success", "Scheduled job updated");
		emit("changed");
	} catch {
		// update errors surface via the global error toast
	} finally {
		saving.value = false;
	}
}

function jobStatusSeverity(s: string): string {
	return s === "ACTIVE" ? "success" : s === "PAUSED" ? "warn" : "secondary";
}
function instanceStatusSeverity(s: string): string {
	switch (s) {
		case "DELIVERED":
		case "COMPLETED":
			return "success";
		case "QUEUED":
		case "IN_FLIGHT":
			return "info";
		case "FAILED":
		case "DELIVERY_FAILED":
			return "danger";
		default:
			return "secondary";
	}
}
function fmt(s?: string): string {
	return s ? new Date(s).toLocaleString() : "—";
}

async function pause() {
	if (!job.value) return;
	const jobId = job.value.id;
	acting.value = true;
	try {
		await scheduledJobsApi.pause(jobId);
		toast.success("Paused", "Scheduled job paused");
		emit("changed");
		await load(jobId);
	} finally {
		acting.value = false;
	}
}

async function resume() {
	if (!job.value) return;
	const jobId = job.value.id;
	acting.value = true;
	try {
		await scheduledJobsApi.resume(jobId);
		toast.success("Resumed", "Scheduled job resumed");
		emit("changed");
		await load(jobId);
	} finally {
		acting.value = false;
	}
}

async function fireNow() {
	if (!job.value) return;
	const jobId = job.value.id;
	acting.value = true;
	try {
		const result = await scheduledJobsApi.fire(jobId);
		toast.success("Fired", `Instance ${result.id} created`);
		emit("changed");
		await load(jobId);
	} finally {
		acting.value = false;
	}
}

function archive() {
	if (!job.value) return;
	confirm.require({
		message:
			"Archive this scheduled job? It will stop firing but its history is preserved.",
		header: "Confirm Archive",
		icon: "pi pi-exclamation-triangle",
		accept: async () => {
			acting.value = true;
			try {
				await scheduledJobsApi.archive(job.value!.id);
				toast.success("Archived", "Scheduled job archived");
				emit("changed");
				void drawer.value?.close(true);
			} finally {
				acting.value = false;
			}
		},
	});
}

function deleteJob() {
	if (!job.value) return;
	confirm.require({
		message:
			"DELETE this scheduled job? Definition is removed permanently. Instance history is retained until partition retention drops it.",
		header: "Confirm Delete",
		icon: "pi pi-exclamation-triangle",
		acceptClass: "p-button-danger",
		accept: async () => {
			acting.value = true;
			try {
				await scheduledJobsApi.delete(job.value!.id);
				toast.success("Deleted", "Scheduled job deleted");
				emit("changed");
				void drawer.value?.close(true);
			} finally {
				acting.value = false;
			}
		},
	});
}

// Instances are full pages, not drawers — navigate away from the list+drawer.
function viewAllInstances() {
	if (!id.value) return;
	void router.push(`/scheduled-jobs/${id.value}/instances`);
}

function onRecentRowClick(event: { data: ScheduledJobInstance }) {
	void router.push(`/scheduled-jobs/instances/${event.data.id}`);
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    :title="job?.name || 'Scheduled Job'"
    :subtitle="job?.code"
    size="wide"
    :loading="loading"
    :error="loadError"
    :dirty="editing && dirty"
    @close="goToList()"
  >
    <template v-if="job" #header-extra>
      <Tag :value="job.status" :severity="jobStatusSeverity(job.status)" />
      <Tag v-if="job.hasActiveInstance" value="Running" severity="warn" />
    </template>

    <template v-if="job">
      <!-- Definition -->
      <FcFormSection title="Definition" flat>
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
                <Textarea :id="fieldId" v-model="editDescription" rows="2" />
              </template>
            </FcFormField>
            <FcFormField label="Timezone">
              <template #default="{ id: fieldId }">
                <InputText :id="fieldId" v-model="editTimezone" placeholder="UTC" />
              </template>
            </FcFormField>
            <FcFormField label="Delivery Max Attempts">
              <template #default="{ id: fieldId }">
                <InputNumber :inputId="fieldId" v-model="editDeliveryMaxAttempts" :min="1" :max="20" />
              </template>
            </FcFormField>
            <FcFormField label="Timeout Seconds">
              <template #default="{ id: fieldId }">
                <InputNumber :inputId="fieldId" v-model="editTimeoutSeconds" :min="1" placeholder="—" />
              </template>
            </FcFormField>
            <FcFormField label="Target URL" span>
              <template #default="{ id: fieldId }">
                <InputText :id="fieldId" v-model="editTargetUrl" placeholder="https://app.example.com/_fc/scheduled-jobs/process" />
              </template>
            </FcFormField>
            <FcFormField label="Cron Expressions" span>
              <template #default>
                <div v-for="(_, idx) in editCrons" :key="idx" class="cron-row">
                  <InputText v-model="editCrons[idx]" placeholder="0 0 * * * *" class="cron-input font-mono" />
                  <Button
                    icon="pi pi-trash"
                    severity="danger"
                    text
                    :disabled="editCrons.length === 1"
                    @click="removeEditCron(idx)"
                  />
                </div>
                <Button label="Add another" icon="pi pi-plus" text size="small" @click="addEditCron" />
              </template>
            </FcFormField>
            <FcFormField label="Payload (JSON)" span>
              <template #default="{ id: fieldId }">
                <Textarea :id="fieldId" v-model="editPayloadJson" rows="5" class="font-mono" placeholder="{}" />
              </template>
            </FcFormField>
            <FcFormField label="Options" span>
              <template #default>
                <div class="toggle-row">
                  <Checkbox v-model="editConcurrent" :binary="true" input-id="editConcurrent" />
                  <label for="editConcurrent" class="toggle-label">Allow concurrent firings</label>
                </div>
                <div class="toggle-row">
                  <Checkbox v-model="editTracksCompletion" :binary="true" input-id="editTracksCompletion" />
                  <label for="editTracksCompletion" class="toggle-label">SDK reports completion</label>
                </div>
              </template>
            </FcFormField>
          </div>
        </template>

        <template v-else>
          <div class="fc-detail-grid">
            <FcDetailField label="Code">
              <code>{{ job.code }}</code>
            </FcDetailField>
            <FcDetailField label="Scope" :value="job.clientName ?? job.clientId ?? 'Platform'" />
            <FcDetailField label="Application" :value="job.applicationName ?? '—'" />
            <FcDetailField label="Timezone" :value="job.timezone" />
            <FcDetailField label="Last Fired" :value="fmt(job.lastFiredAt)" />
            <FcDetailField label="Created" :value="fmt(job.createdAt)" />
            <FcDetailField label="Updated" :value="fmt(job.updatedAt)" />
            <FcDetailField label="Delivery Max Attempts" :value="job.deliveryMaxAttempts" />
            <FcDetailField label="Timeout Seconds" :value="job.timeoutSeconds" />
            <FcDetailField label="Concurrent" :value="job.concurrent ? 'Yes' : 'No'" />
            <FcDetailField label="Tracks Completion" :value="job.tracksCompletion ? 'Yes' : 'No'" />
            <FcDetailField label="Target URL" span>
              <code v-if="job.targetUrl" class="target-url">{{ job.targetUrl }}</code>
              <span v-else class="missing-target">— not configured (firings will fail)</span>
            </FcDetailField>
            <FcDetailField label="Cron Expressions" span>
              <ul class="cron-list">
                <li v-for="(c, i) in job.crons" :key="i"><code>{{ c }}</code></li>
              </ul>
            </FcDetailField>
            <FcDetailField
              v-if="job.description"
              label="Description"
              :value="job.description"
              span
            />
            <FcDetailField v-if="job.payload" label="Payload" span>
              <pre class="payload-pre">{{ JSON.stringify(job.payload, null, 2) }}</pre>
            </FcDetailField>
          </div>
        </template>
      </FcFormSection>

      <!-- Recent Firings -->
      <FcFormSection title="Recent Firings" flat>
        <template #actions>
          <Button
            label="View all"
            icon="pi pi-arrow-right"
            icon-pos="right"
            text
            @click="viewAllInstances"
          />
        </template>
        <DataTable
          :value="recentInstances"
          row-hover
          selection-mode="single"
          @row-click="onRecentRowClick"
        >
          <Column header="Fired At">
            <template #body="{ data }">{{ fmt(data.firedAt) }}</template>
          </Column>
          <Column header="Trigger" field="triggerKind" />
          <Column header="Status">
            <template #body="{ data }">
              <Tag :value="data.status" :severity="instanceStatusSeverity(data.status)" />
            </template>
          </Column>
          <Column header="Attempts" field="deliveryAttempts" />
          <Column header="Delivered">
            <template #body="{ data }">{{ fmt(data.deliveredAt) }}</template>
          </Column>
          <Column header="Completed">
            <template #body="{ data }">{{ fmt(data.completedAt) }}</template>
          </Column>
          <template #empty>
            <div class="empty-message">No firings yet.</div>
          </template>
        </DataTable>
      </FcFormSection>

      <!-- Actions -->
      <FcFormSection v-if="!editing" title="Actions" flat>
        <div class="action-buttons">
          <Button
            v-if="job.status === 'ACTIVE'"
            label="Pause"
            icon="pi pi-pause"
            severity="warn"
            :loading="acting"
            @click="pause"
          />
          <Button
            v-if="job.status === 'PAUSED'"
            label="Resume"
            icon="pi pi-play"
            severity="success"
            :loading="acting"
            @click="resume"
          />
          <Button
            label="Fire Now"
            icon="pi pi-bolt"
            :disabled="job.status === 'ARCHIVED'"
            :loading="acting"
            @click="fireNow"
          />
          <Button
            v-if="job.status !== 'ARCHIVED'"
            label="Archive"
            icon="pi pi-inbox"
            severity="secondary"
            :loading="acting"
            @click="archive"
          />
          <Button
            label="Delete"
            icon="pi pi-trash"
            severity="danger"
            text
            :loading="acting"
            @click="deleteJob"
          />
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
.target-url {
  font-size: 13px;
  word-break: break-all;
}

.missing-target {
  color: #f97316;
}

.cron-list {
  margin: 0;
  padding-left: 18px;
  font-size: 13px;
}

.payload-pre {
  margin: 0;
  padding: 8px;
  font-size: 12px;
  background: #f8fafc;
  border-radius: 6px;
  overflow-x: auto;
}

.action-buttons {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.empty-message {
  text-align: center;
  padding: 16px;
  color: #64748b;
}

.cron-row {
  display: flex;
  gap: 8px;
  align-items: center;
  margin-bottom: 8px;
}

.cron-input {
  flex: 1;
}

.font-mono {
  font-family: "SF Mono", "Consolas", monospace;
}

.toggle-row {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 8px;
}

.toggle-row:last-child {
  margin-bottom: 0;
}

.toggle-label {
  font-weight: 500;
  cursor: pointer;
}
</style>
