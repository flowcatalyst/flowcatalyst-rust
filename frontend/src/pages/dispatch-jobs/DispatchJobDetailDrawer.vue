<script setup lang="ts">
import { computed, ref, watch } from "vue";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";
import {
	dispatchJobsApi,
	type DeliveryPlan,
	type DeliveryRequestSummary,
	type DispatchJobAttempt,
	type DispatchJobDetail,
} from "@/api/dispatch-jobs";
import { toast } from "@/utils/errorBus";

// Read-only working panel: the job, its payload, and every attempt with
// what was SENT (signing account, headers) beside what came BACK (status,
// body). The "Sign" action builds the delivery as it would go out right
// now — a real signature over the real body — without sending it, so an
// operator can hand timestamp + signature + body to the subscriber's
// verify command and see which side holds the wrong secret.
const { id, goToList } = useDrawerRoute({ listPath: "/dispatch-jobs" });

const job = ref<DispatchJobDetail | null>(null);
const attempts = ref<DispatchJobAttempt[]>([]);
const loading = ref(false);
const loadError = ref<string | null>(null);

const plan = ref<DeliveryPlan | null>(null);
const signing = ref(false);
const requeuing = ref(false);

watch(id, load, { immediate: true });

async function load() {
	if (!id.value) return;
	loading.value = true;
	loadError.value = null;
	job.value = null;
	attempts.value = [];
	plan.value = null;
	try {
		const [j, a] = await Promise.all([
			dispatchJobsApi.get(id.value),
			dispatchJobsApi.attempts(id.value),
		]);
		job.value = j;
		// The API lists attempts oldest-first; an operator reads the latest
		// answer first. Position, not the stored attemptNumber, is the label:
		// every requeue starts a new run numbered from 1 again.
		attempts.value = [...a].reverse();
	} catch (error) {
		console.error("Failed to load dispatch job:", error);
		loadError.value = "Failed to load dispatch job.";
	} finally {
		loading.value = false;
	}
}

async function sign() {
	if (!id.value || signing.value) return;
	signing.value = true;
	try {
		plan.value = await dispatchJobsApi.sign(id.value);
	} catch (error) {
		toast.error("Sign failed", error instanceof Error ? error.message : undefined);
	} finally {
		signing.value = false;
	}
}

async function requeue() {
	if (!id.value || requeuing.value) return;
	requeuing.value = true;
	try {
		const { requeued } = await dispatchJobsApi.requeue([id.value]);
		toast.success("Requeued", `${requeued} dispatch job reset to PENDING`);
		await load();
	} catch (error) {
		toast.error("Requeue failed", error instanceof Error ? error.message : undefined);
	} finally {
		requeuing.value = false;
	}
}

async function copy(label: string, value: string | undefined) {
	if (!value) return;
	try {
		await navigator.clipboard.writeText(value);
		toast.success("Copied", label);
	} catch {
		toast.error("Copy failed");
	}
}

// The three the subscriber's verify command takes, as one paste.
const verifyCommand = computed(() => {
	if (!plan.value) return "";
	const ts = plan.value.headers["X-FlowCatalyst-Timestamp"] ?? "";
	const sig = plan.value.headers["X-FlowCatalyst-Signature"] ?? "";
	return `php artisan flowcatalyst:verify-signature --timestamp='${ts}' --signature='${sig}' --body-file=body.json`;
});

function formatDate(dateStr: string | undefined): string {
	if (!dateStr) return "-";
	return new Date(dateStr).toLocaleString();
}

function formatJson(data: string | undefined | null): string {
	if (data == null || data === "") return "-";
	try {
		return JSON.stringify(JSON.parse(data), null, 2);
	} catch {
		return data;
	}
}

function attemptSeverity(a: DispatchJobAttempt): "success" | "danger" | "warn" {
	if (a.success) return "success";
	const code = a.responseCode ?? 0;
	return code === 429 || (code >= 200 && code < 300) ? "warn" : "danger";
}

function requestLine(r: DeliveryRequestSummary | undefined): string {
	if (!r) return "not recorded";
	if (r.unsignedReason) return `UNSIGNED — ${r.unsignedReason}`;
	const parts: string[] = [];
	if (r.signature) parts.push(`signed by ${r.signedBy || "?"}`);
	if (r.bearer) parts.push("bearer");
	return parts.join(" + ") || "no credentials";
}

function statusSeverity(status: string | undefined) {
	switch (status) {
		case "COMPLETED":
			return "success";
		case "FAILED":
			return "danger";
		case "PROCESSING":
			return "warn";
		case "CANCELLED":
		case "EXPIRED":
			return "secondary";
		default:
			return "info";
	}
}
</script>

<template>
  <EntityDrawer
    :title="job?.descriptor || job?.code || 'Dispatch job'"
    :subtitle="id"
    size="two-thirds"
    :loading="loading"
    :error="loadError"
    @close="goToList()"
  >
    <div v-if="job" class="job-detail">
      <div class="toolbar">
        <Tag :value="job.status" :severity="statusSeverity(job.status)" />
        <span class="spacer" />
        <Button
          label="Sign"
          icon="pi pi-key"
          size="small"
          severity="secondary"
          :loading="signing"
          v-tooltip="'Build this delivery as it would go out now — signing account, headers, a real signature — without sending it'"
          @click="sign"
        />
        <Button
          label="Requeue"
          icon="pi pi-replay"
          size="small"
          severity="secondary"
          :loading="requeuing"
          v-tooltip="'Reset to PENDING for re-dispatch'"
          @click="requeue"
        />
      </div>

      <div class="columns">
        <section class="col">
          <h3>Job</h3>
          <div class="detail-row"><label>Descriptor</label><span>{{ job.descriptor || '-' }}</span></div>
          <div class="detail-row"><label>Code</label><span class="font-mono">{{ job.code }}</span></div>
          <div class="detail-row"><label>Client</label><span class="font-mono">{{ job.clientId || '-' }}</span></div>
          <div class="detail-row"><label>Message group</label><span class="font-mono">{{ job.messageGroup || '-' }}</span></div>
          <div class="detail-row"><label>Mode</label><span>{{ job.mode }}</span></div>
          <div class="detail-row"><label>Target</label><span class="font-mono break">{{ job.targetUrl }}</span></div>
          <div class="detail-row"><label>Subscription</label><span class="font-mono">{{ job.subscriptionId || '-' }}</span></div>
          <div class="detail-row"><label>Event</label><span class="font-mono">{{ job.eventId || '-' }}</span></div>
          <div class="detail-row"><label>Attempts</label><span>{{ job.attemptCount }} / {{ job.maxRetries }}</span></div>
          <div class="detail-row"><label>Scheduled for</label><span>{{ formatDate(job.scheduledFor) }}</span></div>
          <div class="detail-row"><label>Created</label><span>{{ formatDate(job.createdAt) }}</span></div>
          <div class="detail-row"><label>Completed</label><span>{{ formatDate(job.completedAt) }}</span></div>
          <div v-if="job.lastError" class="detail-row"><label>Last error</label><span class="error-text">{{ job.lastError }}</span></div>

          <div v-if="job.metadata?.length" class="detail-section">
            <label>Additional data</label>
            <div class="kv">
              <div v-for="m in job.metadata" :key="m.key" class="kv-item">
                <span class="kv-key">{{ m.key }}</span>
                <span class="kv-value">{{ m.value }}</span>
              </div>
            </div>
          </div>

          <div class="detail-section">
            <div class="section-head">
              <label>Payload</label>
              <Button icon="pi pi-copy" text size="small" v-tooltip="'Copy payload'" @click="copy('Payload', job.payload ?? undefined)" />
            </div>
            <pre class="data-block">{{ formatJson(job.payload) }}</pre>
          </div>
        </section>

        <section class="col">
          <h3>Attempts</h3>
          <p v-if="!attempts.length" class="text-muted">No attempts yet.</p>
          <div v-for="(a, i) in attempts" :key="`${a.attemptedAt}-${a.attemptNumber}`" class="attempt">
            <div class="attempt-head">
              <Tag
                :value="`#${attempts.length - i}`"
                severity="secondary"
                v-tooltip="`Attempt ${a.attemptNumber} of its run — a requeue starts a new run numbered from 1`"
              />
              <Tag :value="a.responseCode ? String(a.responseCode) : (a.errorType || 'no response')" :severity="attemptSeverity(a)" />
              <span class="text-sm">{{ formatDate(a.attemptedAt) }}</span>
              <span v-if="a.durationMillis != null" class="text-sm text-muted">{{ a.durationMillis }}ms</span>
            </div>
            <div class="detail-row"><label>Sent</label><span>{{ requestLine(a.request) }}</span></div>
            <div v-if="a.request?.timestamp" class="detail-row"><label>Timestamp</label><span class="font-mono">{{ a.request.timestamp }}</span></div>
            <div v-if="a.request?.headers?.length" class="detail-row"><label>Headers</label><span class="font-mono small">{{ a.request.headers.join(', ') }}</span></div>
            <div v-if="a.errorMessage" class="detail-row"><label>Error</label><span class="error-text">{{ a.errorMessage }}</span></div>
            <div v-if="a.responseBody" class="detail-section">
              <label>Response</label>
              <pre class="data-block small">{{ formatJson(a.responseBody) }}</pre>
            </div>
          </div>

          <div v-if="plan" class="plan">
            <h3>Delivery as it would go out now</h3>
            <div class="detail-row"><label>Sent</label><span>{{ requestLine(plan.request) }}</span></div>
            <div class="detail-row"><label>Target</label><span class="font-mono break">{{ plan.request.target }}</span></div>
            <div class="detail-section">
              <label>Headers</label>
              <div class="kv">
                <div v-for="(v, k) in plan.headers" :key="k" class="kv-item">
                  <span class="kv-key">{{ k }}</span>
                  <span class="kv-value font-mono break">{{ v }}</span>
                  <Button v-if="k !== 'Authorization'" icon="pi pi-copy" text size="small" @click="copy(String(k), v)" />
                </div>
              </div>
            </div>
            <div class="detail-section">
              <div class="section-head">
                <label>Verify on the subscriber</label>
                <Button icon="pi pi-copy" text size="small" v-tooltip="'Copy command'" @click="copy('Verify command', verifyCommand)" />
              </div>
              <p class="text-sm text-muted">Save the body below as <code>body.json</code>, then in the Laravel app:</p>
              <pre class="data-block small">{{ verifyCommand }}</pre>
            </div>
            <div class="detail-section">
              <div class="section-head">
                <label>Body (exactly as signed)</label>
                <Button icon="pi pi-copy" text size="small" v-tooltip="'Copy body — byte-for-byte, the signature covers it'" @click="copy('Body', plan.body)" />
              </div>
              <pre class="data-block small">{{ plan.body }}</pre>
            </div>
          </div>
        </section>
      </div>
    </div>
  </EntityDrawer>
</template>

<style scoped>
.job-detail { display: flex; flex-direction: column; gap: 1rem; }
.toolbar { display: flex; align-items: center; gap: 0.5rem; }
.spacer { flex: 1; }
/* Attempts are what an operator reads; the job facts + payload are reference. */
.columns { display: grid; grid-template-columns: minmax(0, 5fr) minmax(0, 7fr); gap: 2rem; align-items: start; }
@media (max-width: 1100px) { .columns { grid-template-columns: minmax(0, 1fr); } }
.col { display: flex; flex-direction: column; gap: 0.5rem; min-width: 0; }
h3 { font-size: 0.9375rem; font-weight: 600; margin: 0 0 0.25rem; color: var(--text-color); }
.detail-row { display: flex; gap: 0.75rem; align-items: baseline; }
.detail-row label { flex: 0 0 7rem; font-size: 0.8125rem; color: var(--text-color-secondary); }
.detail-row span { min-width: 0; }
.detail-section { display: flex; flex-direction: column; gap: 0.25rem; margin-top: 0.5rem; }
.detail-section > label, .section-head > label { font-size: 0.8125rem; color: var(--text-color-secondary); }
.section-head { display: flex; align-items: center; justify-content: space-between; }
.data-block {
  margin: 0; padding: 0.75rem; border-radius: 6px; overflow: auto; max-height: 40vh;
  background: var(--surface-ground); font-size: 0.8125rem; white-space: pre-wrap; word-break: break-all;
}
.data-block.small { max-height: 14rem; }
.kv { display: flex; flex-direction: column; gap: 0.25rem; }
.kv-item { display: flex; gap: 0.5rem; align-items: center; font-size: 0.8125rem; }
.kv-key { color: var(--text-color-secondary); flex: 0 0 auto; }
.kv-value { min-width: 0; }
.attempt { border: 1px solid var(--surface-border); border-radius: 6px; padding: 0.75rem; display: flex; flex-direction: column; gap: 0.375rem; }
.attempt-head { display: flex; align-items: center; gap: 0.5rem; }
.plan { border: 1px solid var(--primary-color); border-radius: 6px; padding: 0.75rem; display: flex; flex-direction: column; gap: 0.375rem; margin-top: 0.5rem; }
.font-mono { font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace; }
.break { word-break: break-all; }
.small { font-size: 0.75rem; }
.text-sm { font-size: 0.8125rem; }
.text-muted { color: var(--text-color-secondary); }
.error-text { color: var(--red-600, #dc2626); }
</style>
