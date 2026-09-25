<script setup lang="ts">
import { ref, watch } from "vue";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";
import { eventsApi, type EventDetail } from "@/api/events";

// Read-only drawer — no dirty state, no footer.
const { id, goToList } = useDrawerRoute({ listPath: "/events" });

const event = ref<EventDetail | null>(null);
const loading = ref(false);
const loadError = ref<string | null>(null);

watch(id, load, { immediate: true });

async function load() {
	if (!id.value) return;
	loading.value = true;
	loadError.value = null;
	event.value = null;
	try {
		event.value = await eventsApi.get(id.value);
	} catch (error) {
		console.error("Failed to load event details:", error);
		loadError.value = "Failed to load event details.";
	} finally {
		loading.value = false;
	}
}

function formatDate(dateStr: string | undefined): string {
	if (!dateStr) return "-";
	return new Date(dateStr).toLocaleString();
}

function formatData(data: unknown): string {
	if (data == null) return "-";
	if (typeof data === "object") {
		try {
			return JSON.stringify(data, null, 2);
		} catch {
			return String(data);
		}
	}
	if (typeof data !== "string") return String(data);
	try {
		return JSON.stringify(JSON.parse(data), null, 2);
	} catch {
		return data;
	}
}
</script>

<template>
  <EntityDrawer
    :title="event?.type || 'Event'"
    :subtitle="id"
    size="wide"
    :loading="loading"
    :error="loadError"
    @close="goToList()"
  >
    <div v-if="event" class="event-detail">
      <div class="detail-row">
        <label>ID</label>
        <span class="font-mono">{{ event.id }}</span>
      </div>
      <div class="detail-row">
        <label>Type</label>
        <Tag :value="event.type" severity="info" />
      </div>
      <div class="detail-row">
        <label>Application</label>
        <span>{{ event.application || '-' }}</span>
      </div>
      <div class="detail-row">
        <label>Subdomain</label>
        <span>{{ event.subdomain || '-' }}</span>
      </div>
      <div class="detail-row">
        <label>Aggregate</label>
        <span>{{ event.aggregate || '-' }}</span>
      </div>
      <div class="detail-row">
        <label>Source</label>
        <span>{{ event.source }}</span>
      </div>
      <div class="detail-row">
        <label>Subject</label>
        <span>{{ event.subject || '-' }}</span>
      </div>
      <div class="detail-row">
        <label>Time</label>
        <span>{{ formatDate(event.time) }}</span>
      </div>
      <div class="detail-row">
        <label>Client ID</label>
        <span v-if="event.clientId" class="font-mono">{{ event.clientId }}</span>
        <span v-else class="text-muted">-</span>
      </div>
      <div class="detail-row">
        <label>Message Group</label>
        <span>{{ event.messageGroup || '-' }}</span>
      </div>
      <div class="detail-row">
        <label>Correlation ID</label>
        <span class="font-mono">{{ event.correlationId || '-' }}</span>
      </div>
      <div class="detail-row">
        <label>Causation ID</label>
        <span class="font-mono">{{ event.causationId || '-' }}</span>
      </div>
      <div class="detail-row">
        <label>Deduplication ID</label>
        <span class="font-mono">{{ event.deduplicationId || '-' }}</span>
      </div>
      <div class="detail-row">
        <label>Projected At</label>
        <span>{{ formatDate(event.projectedAt) }}</span>
      </div>
      <div class="detail-section">
        <label>Data</label>
        <pre class="data-block">{{ formatData(event.data) }}</pre>
      </div>
      <div v-if="event.contextData?.length" class="detail-section">
        <label>Context Data</label>
        <div class="context-data">
          <div v-for="cd in event.contextData" :key="cd.key" class="context-item">
            <span class="context-key">{{ cd.key }}:</span>
            <span class="context-value">{{ cd.value }}</span>
          </div>
        </div>
      </div>
    </div>
  </EntityDrawer>
</template>

<style scoped>
.event-detail {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.detail-row {
  display: flex;
  gap: 1rem;
}

.detail-row label {
  font-weight: 600;
  min-width: 120px;
  color: var(--text-color-secondary);
}

.detail-section {
  margin-top: 0.5rem;
}

.detail-section label {
  display: block;
  font-weight: 600;
  margin-bottom: 0.5rem;
  color: var(--text-color-secondary);
}

.data-block {
  background: var(--surface-ground);
  border: 1px solid var(--surface-border);
  border-radius: 6px;
  padding: 1rem;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 0.875rem;
  overflow-x: auto;
  max-height: 300px;
  white-space: pre-wrap;
  word-break: break-word;
}

.context-data {
  background: var(--surface-ground);
  border: 1px solid var(--surface-border);
  border-radius: 6px;
  padding: 0.75rem;
}

.context-item {
  padding: 0.25rem 0;
}

.context-key {
  font-weight: 500;
  margin-right: 0.5rem;
}

.context-value {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.font-mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.text-muted {
  color: var(--text-color-secondary);
}
</style>
