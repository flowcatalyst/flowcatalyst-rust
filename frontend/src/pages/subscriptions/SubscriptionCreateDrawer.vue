<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, onMounted, watch } from "vue";
import {
	subscriptionsApi,
	type SubscriptionMode,
	type EventTypeBinding,
} from "@/api/subscriptions";
import { dispatchPoolsApi, type DispatchPool } from "@/api/dispatch-pools";
import { connectionsApi, type Connection } from "@/api/connections";
import { eventTypesApi, type EventType } from "@/api/event-types";
import ConnectionCreateDialog from "@/components/ConnectionCreateDialog.vue";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";

const emit = defineEmits<{
	changed: [];
}>();

// Form fields
const code = ref("");
const name = ref("");
const description = ref("");
const endpoint = ref("");
const connectionId = ref<string | null>(null);
const queue = ref("");
const maxAgeSeconds = ref<number>(86400);
const delaySeconds = ref<number>(0);
const sequence = ref<number>(99);
const timeoutSeconds = ref<number>(30);
// Matches the platform default: a message group is delivered in sequence,
// moving on past a failure. IMMEDIATE is the explicit opt-out of ordering.
const mode = ref<SubscriptionMode>("NEXT_ON_ERROR");
const dispatchPoolId = ref<string | null>(null);
const clientScoped = ref(false);
const clientId = ref<string | null>(null);
const selectedEventTypes = ref<string[]>([]);

// Cheap dirty check: anything typed or selected counts.
const dirty = computed(
	() =>
		code.value !== "" ||
		name.value !== "" ||
		endpoint.value !== "" ||
		queue.value !== "" ||
		selectedEventTypes.value.length > 0 ||
		dispatchPoolId.value !== null,
);

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { goToList, replaceToDetail } = useDrawerRoute({
	listPath: "/subscriptions",
	dirty,
});

// Lookup data
const dispatchPools = ref<DispatchPool[]>([]);
const connections = ref<Connection[]>([]);
const loadingConnections = ref(true);
const eventTypes = ref<EventType[]>([]);
const loadingPools = ref(true);
const loadingEventTypes = ref(true);
const submitting = ref(false);
const errorMessage = ref<string | null>(null);
const showCreateConnectionDialog = ref(false);

const CODE_PATTERN = /^[a-z][a-z0-9-]*$/;

const modeOptions = [
	{ label: "Immediate", value: "IMMEDIATE" },
	{ label: "Next on Error", value: "NEXT_ON_ERROR" },
	{ label: "Block on Error", value: "BLOCK_ON_ERROR" },
];

const isCodeValid = computed(() => {
	return !code.value || CODE_PATTERN.test(code.value);
});

const isEndpointValid = computed(() => {
	if (!endpoint.value) return true;
	try {
		const url = new URL(endpoint.value);
		return url.protocol === "http:" || url.protocol === "https:";
	} catch {
		return false;
	}
});

const isFormValid = computed(() => {
	return (
		code.value.length >= 2 &&
		code.value.length <= 100 &&
		CODE_PATTERN.test(code.value) &&
		name.value.trim().length > 0 &&
		name.value.length <= 255 &&
		endpoint.value.length > 0 &&
		isEndpointValid.value &&
		queue.value.trim().length > 0 &&
		dispatchPoolId.value !== null &&
		selectedEventTypes.value.length > 0 &&
		maxAgeSeconds.value >= 1 &&
		timeoutSeconds.value >= 1
	);
});

// Filtered event types with CURRENT spec versions for selection
// Only show event types that match the subscription's clientScoped setting
const eventTypeOptions = computed(() => {
	return eventTypes.value
		.filter((et) => et.specVersions.some((sv) => sv.status === "CURRENT"))
		.filter((et) => et.clientScoped === clientScoped.value)
		.map((et) => ({
			id: et.id,
			code: et.code,
			name: et.name,
			currentVersion:
				et.specVersions.find((sv) => sv.status === "CURRENT")?.version || "",
		}));
});

// Clear selected event types when clientScoped changes
watch(clientScoped, () => {
	selectedEventTypes.value = [];
	// Also clear clientId when switching to non-client-scoped
	if (!clientScoped.value) {
		clientId.value = null;
	}
});

onMounted(async () => {
	await Promise.all([loadDispatchPools(), loadEventTypes(), loadConnections()]);
});

async function loadDispatchPools() {
	loadingPools.value = true;
	try {
		const response = await dispatchPoolsApi.list({ status: "ACTIVE" });
		dispatchPools.value = response.pools;
	} catch (e) {
		console.error("Failed to load dispatch pools:", e);
	} finally {
		loadingPools.value = false;
	}
}

async function loadEventTypes() {
	loadingEventTypes.value = true;
	try {
		const response = await eventTypesApi.list({ status: "CURRENT" });
		eventTypes.value = response.items;
	} catch (e) {
		console.error("Failed to load event types:", e);
	} finally {
		loadingEventTypes.value = false;
	}
}

async function loadConnections() {
	loadingConnections.value = true;
	try {
		const response = await connectionsApi.list({ status: "ACTIVE" });
		connections.value = response.connections;
	} catch (e) {
		console.error("Failed to load connections:", e);
	} finally {
		loadingConnections.value = false;
	}
}

function onConnectionCreated(connection: Connection) {
	connections.value.push(connection);
	connectionId.value = connection.id;
}

function buildEventTypeBindings(): EventTypeBinding[] {
	return selectedEventTypes.value.map((etId) => {
		const et = eventTypeOptions.value.find((e) => e.id === etId);
		return {
			eventTypeId: etId,
			eventTypeCode: et?.code || "",
			specVersion: et?.currentVersion || "",
		};
	});
}

async function onSubmit() {
	if (!isFormValid.value) return;

	submitting.value = true;
	errorMessage.value = null;

	try {
		const subscription = await subscriptionsApi.create({
			code: code.value,
			name: name.value,
			description: description.value || undefined,
			endpoint: endpoint.value,
			clientScoped: clientScoped.value,
			connectionId: connectionId.value || undefined,
			queue: queue.value,
			eventTypes: buildEventTypeBindings(),
			dispatchPoolId: dispatchPoolId.value!,
			clientId: clientScoped.value ? clientId.value || undefined : undefined,
			maxAgeSeconds: maxAgeSeconds.value,
			delaySeconds: delaySeconds.value,
			sequence: sequence.value,
			timeoutSeconds: timeoutSeconds.value,
			mode: mode.value,
			source: "UI",
		});
		toast.success("Success", "Subscription created");
		emit("changed");
		replaceToDetail(subscription.id);
	} catch (e) {
		errorMessage.value =
			e instanceof Error ? e.message : "Failed to create subscription";
	} finally {
		submitting.value = false;
	}
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    title="Create Subscription"
    subtitle="Configure a new event subscription for webhook delivery"
    :dirty="dirty"
    @close="goToList()"
  >
    <div class="form-section">
      <h3>Basic Information</h3>

      <div class="form-field">
        <label>Code <span class="required">*</span></label>
        <InputText
          v-model="code"
          placeholder="subscription-code"
          class="full-width"
          :invalid="!!(code && !isCodeValid)"
        />
        <small v-if="code && !isCodeValid" class="p-error">
          Lowercase letters, numbers, hyphens only. Must start with a letter.
        </small>
        <small v-else class="hint">
          Unique identifier for this subscription (2-100 characters)
        </small>
      </div>

      <div class="form-field">
        <label>Name <span class="required">*</span></label>
        <InputText
          v-model="name"
          placeholder="Subscription display name"
          class="full-width"
          :invalid="name.length > 255"
        />
        <small class="char-count">{{ name.length }} / 255</small>
      </div>

      <div class="form-field">
        <label>Description</label>
        <Textarea
          v-model="description"
          placeholder="Optional description..."
          class="full-width"
          rows="3"
        />
      </div>
    </div>

    <div class="form-section">
      <h3>Event Types</h3>

      <div class="form-field">
        <label>Event Types <span class="required">*</span></label>
        <MultiSelect
          v-model="selectedEventTypes"
          :options="eventTypeOptions"
          optionLabel="name"
          optionValue="id"
          placeholder="Select event types"
          class="full-width"
          :loading="loadingEventTypes"
          :disabled="loadingEventTypes"
          display="chip"
          filter
        >
          <template #option="{ option }">
            <div class="dropdown-option">
              <span class="option-name">{{ option.name }}</span>
              <span class="option-code">{{ option.code }} (v{{ option.currentVersion }})</span>
            </div>
          </template>
        </MultiSelect>
        <small class="hint">
          Select which event types this subscription will receive. Only
          {{ clientScoped ? 'client-scoped' : 'non-client-scoped' }} event types are shown.
        </small>
      </div>
    </div>

    <div class="form-section">
      <h3>Delivery Configuration</h3>

      <div class="form-field">
        <label>Endpoint URL <span class="required">*</span></label>
        <InputText
          v-model="endpoint"
          placeholder="https://example.com/webhook"
          class="full-width"
          :invalid="!!(endpoint && !isEndpointValid)"
        />
        <small v-if="endpoint && !isEndpointValid" class="p-error">
          Must be a valid HTTP or HTTPS URL
        </small>
        <small v-else class="hint">
          The webhook URL where events will be delivered
        </small>
      </div>

      <div class="form-field">
        <label>Connection</label>
        <div class="field-with-action">
          <Select
            v-model="connectionId"
            :options="connections"
            optionLabel="name"
            optionValue="id"
            placeholder="Select a connection"
            class="flex-grow"
            :loading="loadingConnections"
            :disabled="loadingConnections"
            filter
          >
            <template #option="{ option }">
              <div class="dropdown-option">
                <span class="option-name">{{ option.name }}</span>
                <span class="option-code">{{ option.code }}</span>
              </div>
            </template>
          </Select>
          <Button
            v-tooltip.top="'Create connection'"
            icon="pi pi-plus"
            outlined
            size="small"
            @click="showCreateConnectionDialog = true"
          />
        </div>
        <small class="hint"> The connection used for delivering events </small>
      </div>

      <div class="form-field">
        <label>Queue <span class="required">*</span></label>
        <InputText v-model="queue" placeholder="default" class="full-width" />
        <small class="hint"> Queue name for routing dispatch jobs </small>
      </div>

      <div class="form-field">
        <label>Dispatch Pool <span class="required">*</span></label>
        <Select
          v-model="dispatchPoolId"
          :options="dispatchPools"
          optionLabel="name"
          optionValue="id"
          placeholder="Select a dispatch pool"
          class="full-width"
          :loading="loadingPools"
          :disabled="loadingPools"
        >
          <template #option="{ option }">
            <div class="dropdown-option">
              <span class="option-name">{{ option.name }}</span>
              <span class="option-code"
                >{{ option.code }} ({{ option.rateLimit }}/min,
                {{ option.concurrency }} concurrent)</span
              >
            </div>
          </template>
        </Select>
        <small class="hint"> Rate-limiting pool for this subscription's dispatch jobs </small>
      </div>

      <div class="form-field">
        <label>Mode</label>
        <Select
          v-model="mode"
          :options="modeOptions"
          optionLabel="label"
          optionValue="value"
          class="full-width"
        />
        <small class="hint"> How to handle dispatch failures </small>
      </div>
    </div>

    <div class="form-section">
      <h3>Timing</h3>

      <div class="form-row">
        <div class="form-field">
          <label>Max Age (seconds)</label>
          <InputNumber v-model="maxAgeSeconds" :min="1" class="full-width" />
          <small class="hint">Maximum age before events expire</small>
        </div>

        <div class="form-field">
          <label>Timeout (seconds)</label>
          <InputNumber v-model="timeoutSeconds" :min="1" class="full-width" />
          <small class="hint">Request timeout for webhook calls</small>
        </div>
      </div>

      <div class="form-row">
        <div class="form-field">
          <label>Delay (seconds)</label>
          <InputNumber v-model="delaySeconds" :min="0" class="full-width" />
          <small class="hint">Delay before dispatching</small>
        </div>

        <div class="form-field">
          <label>Sequence</label>
          <InputNumber v-model="sequence" :min="1" class="full-width" />
          <small class="hint">Processing order (lower = earlier)</small>
        </div>
      </div>
    </div>

    <div class="form-section">
      <h3>Scope</h3>

      <div class="form-field toggle-field">
        <div class="toggle-row">
          <ToggleSwitch v-model="clientScoped" inputId="clientScoped" />
          <label for="clientScoped" class="toggle-label">Client Scoped</label>
        </div>
        <small class="hint">
          Client-scoped subscriptions receive events that are specific to individual clients.
          Non-client-scoped subscriptions receive platform-wide events.
        </small>
      </div>

      <div v-if="clientScoped" class="form-field">
        <label>Client</label>
        <ClientSelect
          v-model="clientId"
          placeholder="All clients (leave empty) or select specific client"
        />
        <small class="hint">
          Leave empty to receive events for all clients, or select a specific client.
        </small>
      </div>
    </div>

    <Message v-if="errorMessage" severity="error" class="error-message">
      {{ errorMessage }}
    </Message>

    <ConnectionCreateDialog
      v-model:visible="showCreateConnectionDialog"
      @created="onConnectionCreated"
    />

    <template #footer>
      <FcFormActions :bordered="false">
        <Button
          label="Cancel"
          icon="pi pi-times"
          severity="secondary"
          outlined
          :disabled="submitting"
          @click="drawer?.close()"
        />
        <Button
          label="Create Subscription"
          icon="pi pi-check"
          :loading="submitting"
          :disabled="!isFormValid"
          @click="onSubmit"
        />
      </FcFormActions>
    </template>
  </EntityDrawer>
</template>

<style scoped>
.form-section {
  margin-bottom: 32px;
}

.form-section h3 {
  margin: 0 0 16px 0;
  font-size: 14px;
  font-weight: 600;
  color: #475569;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.form-field {
  margin-bottom: 20px;
}

.form-field > label {
  display: block;
  font-weight: 500;
  margin-bottom: 6px;
}

.form-field .required {
  color: #ef4444;
}

.form-row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 20px;
}

.full-width {
  width: 100%;
}

.field-with-action {
  display: flex;
  gap: 8px;
  align-items: flex-start;
}

.flex-grow {
  flex: 1;
}

.char-count {
  display: block;
  text-align: right;
  font-size: 12px;
  color: #94a3b8;
  margin-top: 4px;
}

.hint {
  display: block;
  font-size: 12px;
  color: #64748b;
  margin-top: 4px;
}

.toggle-field {
  margin-top: 8px;
}

.toggle-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.toggle-label {
  font-weight: 500;
  cursor: pointer;
}

.dropdown-option {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.option-name {
  font-weight: 500;
}

.option-code {
  font-size: 12px;
  color: #64748b;
  font-family: monospace;
}

.error-message {
  margin-bottom: 16px;
}

@media (max-width: 640px) {
  .form-row {
    grid-template-columns: 1fr;
  }
}
</style>
