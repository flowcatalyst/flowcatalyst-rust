<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { toast } from "@/utils/errorBus";
import {
	scheduledJobsApi,
	type CreateScheduledJobBody,
	type FilterOption,
} from "@/api/scheduled-jobs";
import { applicationsApi, type Application } from "@/api/applications";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";

const emit = defineEmits<{
	changed: [];
}>();

const form = ref<CreateScheduledJobBody>({
	code: "",
	name: "",
	description: "",
	clientId: undefined,
	applicationId: undefined,
	crons: [""],
	timezone: "UTC",
	concurrent: false,
	tracksCompletion: false,
	timeoutSeconds: undefined,
	deliveryMaxAttempts: 3,
	targetUrl: "",
});

const payloadJson = ref<string>("");
const submitting = ref(false);
const clientOptions = ref<FilterOption[]>([]);
const applications = ref<Application[]>([]);

// Cheap dirty check over the identity fields: anything typed counts.
const dirty = computed(
	() =>
		form.value.code !== "" ||
		form.value.name !== "" ||
		(form.value.description ?? "") !== "",
);

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { goToList, replaceToDetail } = useDrawerRoute({
	listPath: "/scheduled-jobs",
	dirty,
});

onMounted(async () => {
	try {
		const opts = await scheduledJobsApi.filterOptions();
		clientOptions.value = opts.clients;
	} catch (err) {
		console.error("Failed to load client list", err);
	}
	try {
		const response = await applicationsApi.listApplicationsOnly(true);
		applications.value = response.applications || [];
	} catch (err) {
		console.error("Failed to load application list", err);
	}
});

function addCron() {
	form.value.crons.push("");
}
function removeCron(idx: number) {
	if (form.value.crons.length > 1) form.value.crons.splice(idx, 1);
}

async function submit() {
	if (!form.value.code.trim() || !form.value.name.trim()) {
		toast.warn("Missing fields", "Code and name are required");
		return;
	}
	if (!form.value.clientId) {
		toast.warn(
			"Missing scope",
			"Choose Platform or a client — this can't be changed after creation",
		);
		return;
	}
	const cleanCrons = form.value.crons.map((c) => c.trim()).filter(Boolean);
	if (cleanCrons.length === 0) {
		toast.warn("Missing cron", "At least one cron expression is required");
		return;
	}

	let parsedPayload: unknown = undefined;
	if (payloadJson.value.trim()) {
		try {
			parsedPayload = JSON.parse(payloadJson.value);
		} catch {
			toast.warn("Invalid payload", "Payload must be valid JSON or empty");
			return;
		}
	}

	const body: CreateScheduledJobBody = {
		...form.value,
		crons: cleanCrons,
		clientId:
			form.value.clientId === "platform" || !form.value.clientId
				? null
				: form.value.clientId,
		applicationId: form.value.applicationId || null,
		payload: parsedPayload,
		description: form.value.description?.trim() || undefined,
		targetUrl: form.value.targetUrl?.trim() || undefined,
	};

	submitting.value = true;
	try {
		const result = await scheduledJobsApi.create(body);
		toast.success("Created", `Scheduled job '${form.value.code}' created`);
		emit("changed");
		replaceToDetail(result.id);
	} catch (err) {
		console.error("Failed to create scheduled job", err);
	} finally {
		submitting.value = false;
	}
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    title="New Scheduled Job"
    subtitle="Register a cron-driven job the platform will fire on schedule."
    :dirty="dirty"
    @close="goToList()"
  >
    <section class="form-section">
      <h3 class="section-title">Identity</h3>

      <div class="form-grid">
        <div class="form-field">
          <label>Code <span class="required">*</span></label>
          <InputText v-model="form.code" placeholder="daily-cleanup" class="full-width" />
          <small class="field-hint">Routing key the SDK uses to find the registered handler.</small>
        </div>

        <div class="form-field">
          <label>Name <span class="required">*</span></label>
          <InputText v-model="form.name" placeholder="Daily cleanup job" class="full-width" />
        </div>

        <div class="form-field span-2">
          <label>Description</label>
          <Textarea v-model="form.description" :rows="2" class="full-width" />
        </div>

        <div class="form-field">
          <label>Scope <span class="required">*</span></label>
          <Select
            v-model="form.clientId"
            :options="[{ value: 'platform', label: 'Platform-scoped (anchor only)' }, ...clientOptions]"
            option-label="label"
            option-value="value"
            placeholder="Select a client or platform"
            class="full-width"
          />
          <small class="field-hint">Who owns this job — cannot be changed after creation.</small>
        </div>

        <div class="form-field">
          <label>Application</label>
          <Select
            v-model="form.applicationId"
            :options="applications"
            option-label="name"
            option-value="id"
            placeholder="None"
            show-clear
            class="full-width"
          />
          <small class="field-hint">Optional — for filtering/display only.</small>
        </div>

        <div class="form-field">
          <label>Timezone</label>
          <InputText v-model="form.timezone" placeholder="UTC" class="full-width" />
          <small class="field-hint">IANA name (e.g. <code>Europe/London</code>).</small>
        </div>
      </div>
    </section>

    <section class="form-section">
      <h3 class="section-title">Schedule</h3>

      <div class="form-field">
        <label>Cron Expressions <span class="required">*</span></label>
        <div v-for="(_, idx) in form.crons" :key="idx" class="cron-row">
          <InputText
            v-model="form.crons[idx]"
            placeholder="0 0 * * * *"
            class="full-width font-mono"
          />
          <Button
            icon="pi pi-trash"
            severity="danger"
            text
            :disabled="form.crons.length === 1"
            @click="removeCron(idx)"
          />
        </div>
        <Button label="Add another" icon="pi pi-plus" text size="small" @click="addCron" />
        <small class="field-hint">
          6-field cron syntax (sec min hour day month dow). Multiple expressions
          are unioned at fire time.
        </small>
      </div>
    </section>

    <section class="form-section">
      <h3 class="section-title">Delivery</h3>

      <div class="form-field">
        <label>Target URL</label>
        <InputText
          v-model="form.targetUrl"
          placeholder="https://app.example.com/_fc/scheduled-jobs/process"
          class="full-width"
        />
        <small class="field-hint">
          The platform POSTs the firing envelope here. Without it, instances
          will be marked DELIVERY_FAILED on every fire.
        </small>
      </div>

      <div class="form-grid">
        <div class="form-field">
          <label>Delivery Max Attempts</label>
          <InputNumber v-model="form.deliveryMaxAttempts" :min="1" :max="20" class="full-width" />
        </div>
        <div class="form-field">
          <label>Timeout Seconds (hint)</label>
          <InputNumber v-model="form.timeoutSeconds" :min="1" placeholder="—" class="full-width" />
          <small class="field-hint">Hint passed to the SDK; not enforced by the platform.</small>
        </div>
      </div>

      <div class="toggle-row">
        <Checkbox v-model="form.concurrent" :binary="true" input-id="concurrent" />
        <label for="concurrent" class="toggle-label">Allow concurrent firings</label>
      </div>

      <div class="toggle-row">
        <Checkbox v-model="form.tracksCompletion" :binary="true" input-id="tracksCompletion" />
        <label for="tracksCompletion" class="toggle-label">SDK reports completion</label>
      </div>
    </section>

    <section class="form-section">
      <h3 class="section-title">Payload</h3>

      <div class="form-field">
        <label>Payload (JSON)</label>
        <Textarea
          v-model="payloadJson"
          :rows="5"
          class="full-width font-mono"
          placeholder="{}"
        />
        <small class="field-hint">Optional. Passed verbatim in every firing envelope.</small>
      </div>
    </section>

    <template #footer>
      <FcFormActions :bordered="false">
        <Button
          label="Cancel"
          severity="secondary"
          outlined
          :disabled="submitting"
          @click="drawer?.close()"
        />
        <Button label="Create" icon="pi pi-check" :loading="submitting" @click="submit" />
      </FcFormActions>
    </template>
  </EntityDrawer>
</template>

<style scoped>
.form-section {
  margin-bottom: 32px;
}

.form-section:last-of-type {
  margin-bottom: 16px;
}

.section-title {
  font-size: 16px;
  font-weight: 600;
  margin-bottom: 16px;
}

.form-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
}

.form-field {
  display: flex;
  flex-direction: column;
  margin-bottom: 16px;
}

.form-grid .form-field {
  margin-bottom: 0;
}

.form-field > label {
  font-weight: 500;
  margin-bottom: 6px;
  font-size: 14px;
}

.form-field .required {
  color: #ef4444;
}

.field-hint {
  display: block;
  color: var(--text-color-secondary);
  font-size: 13px;
  margin-top: 6px;
  line-height: 1.4;
}

.field-hint code {
  background: var(--surface-ground);
  padding: 1px 5px;
  border-radius: 3px;
  font-family: monospace;
  font-size: 12px;
}

.span-2 {
  grid-column: span 2;
}

.full-width {
  width: 100%;
}

.font-mono {
  font-family: "SF Mono", "Consolas", monospace;
}

.cron-row {
  display: flex;
  gap: 8px;
  align-items: center;
  margin-bottom: 8px;
}

.cron-row .full-width {
  flex: 1;
}

.toggle-row {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
}

.toggle-label {
  font-weight: 500;
  cursor: pointer;
}

@media (max-width: 640px) {
  .form-grid {
    grid-template-columns: 1fr;
  }

  .span-2 {
    grid-column: span 1;
  }
}
</style>
