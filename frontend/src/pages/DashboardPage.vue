<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useConfirm } from "primevue/useconfirm";
import { useAuthStore } from "@/stores/auth";
import { eventTypesApi } from "@/api/event-types";
import { rolesApi } from "@/api/roles";
import { developerApi } from "@/api/developer";
import { redactExistingAuditLogs } from "@/api/audit-logs";
import { dashboardApi, type DashboardStats } from "@/api/dashboard";
import { toast } from "@/utils/errorBus";
import { canEnterRoute, lacksAnchor, userCan } from "@/stores/permissions";

const authStore = useAuthStore();

// What this user may see here (decision #8): the server enforces each call;
// the dashboard only hides what would be refused.
const user = computed(() => authStore.user);
/** Go's profile-only rule: a user with no role and no permission. */
const hasNoPlatformRole = computed(
	() =>
		!!user.value &&
		user.value.roles.length === 0 &&
		Array.isArray(user.value.permissions) &&
		user.value.permissions.length === 0,
);
const anchorOk = computed(() => !!user.value && !lacksAnchor(user.value));
/** `GET /bff/dashboard/stats`: anchor, then client or application view. */
const canViewStats = computed(
	() =>
		anchorOk.value &&
		userCan(user.value, [
			"platform:admin:client:view",
			"platform:admin:application:view",
		]),
);
const canSyncEvents = computed(
	() =>
		anchorOk.value &&
		userCan(user.value, [
			"platform:messaging:event-type:create",
			"platform:messaging:event-type:update",
			"platform:messaging:event-type:delete",
		]),
);
const canSyncRoles = computed(
	() =>
		anchorOk.value &&
		userCan(user.value, [
			"platform:iam:role:create",
			"platform:iam:role:update",
			"platform:iam:role:delete",
		]),
);
const canSyncOpenApi = computed(
	() =>
		anchorOk.value &&
		userCan(user.value, [
			"platform:developer:application-openapi:sync",
			"platform:developer:application-openapi:manage",
		]),
);
const canRedactAuditLogs = computed(
	() => anchorOk.value && userCan(user.value, "platform:admin:audit-log:view"),
);
const showSync = computed(
	() =>
		canSyncEvents.value ||
		canSyncRoles.value ||
		canSyncOpenApi.value ||
		canRedactAuditLogs.value,
);
const confirm = useConfirm();

const syncingEvents = ref(false);
const syncingRoles = ref(false);
const syncingOpenApi = ref(false);
// TEMPORARY (docs/spec/audit-redaction.md in the Java repo, "Temporary: redact
// existing rows from the dashboard"; remove this card + handler once the sweep is no
// longer needed): deliberately NOT part of `syncAllPlatform` — this is a
// one-off cleanup action on existing rows, not a re-apply-on-every-deploy sync.
const redactingAuditLogs = ref(false);

// Platform Overview stat cards. `null` = loading / failed; rendered as "—".
// One round-trip to /bff/dashboard/stats replaces three separate list
// fetches; the high-volume tables (events, dispatch jobs, audit logs,
// login attempts) come from `pg_class.reltuples` — sub-millisecond regardless
// of row count, accurate to within a few % once autovacuum has analyzed.
const stats = ref<DashboardStats | null>(null);

async function loadStats() {
	try {
		stats.value = await dashboardApi.stats();
	} catch {
		// Toast already surfaced via global interceptor; cards stay at "—".
	}
}

function fmt(n: number | null | undefined): string {
	return n === null || n === undefined ? "—" : n.toLocaleString();
}

/** Format an approximate count with `~` prefix to flag it as estimated. */
function fmtApprox(n: number | null | undefined): string {
	return n === null || n === undefined ? "—" : `~${n.toLocaleString()}`;
}

onMounted(() => {
	if (canViewStats.value) void loadStats();
});

async function syncPlatformEvents() {
	syncingEvents.value = true;
	try {
		const result = await eventTypesApi.syncPlatform();
		const parts: string[] = [];
		if (result.created > 0) parts.push(`${result.created} created`);
		if (result.updated > 0) parts.push(`${result.updated} updated`);
		if (result.deleted > 0) parts.push(`${result.deleted} deleted`);
		toast.success(
			"Platform Events Synced",
			parts.length > 0
				? `${parts.join(", ")} (${result.total} total)`
				: `${result.total} event types up to date`,
		);
	} catch {
	} finally {
		syncingEvents.value = false;
	}
}

async function syncPlatformRoles() {
	syncingRoles.value = true;
	try {
		const result = await rolesApi.syncPlatform();
		const parts: string[] = [];
		if (result.created > 0) parts.push(`${result.created} created`);
		if (result.updated > 0) parts.push(`${result.updated} updated`);
		if (result.removed > 0) parts.push(`${result.removed} removed`);
		toast.success(
			"Platform Roles Synced",
			parts.length > 0
				? `${parts.join(", ")} (${result.total} total)`
				: `${result.total} roles up to date`,
		);
	} catch {
	} finally {
		syncingRoles.value = false;
	}
}

async function syncPlatformOpenApi() {
	syncingOpenApi.value = true;
	try {
		const result = await developerApi.syncPlatformOpenApi();
		if (result.unchanged) {
			toast.success(
				"Platform OpenAPI Synced",
				`No changes (v${result.version} unchanged)`,
			);
		} else if (result.archivedPriorVersion) {
			const breaking = result.hasBreaking ? " · breaking" : "";
			toast.success(
				"Platform OpenAPI Synced",
				`v${result.archivedPriorVersion} → v${result.version}${breaking}`,
			);
		} else {
			toast.success(
				"Platform OpenAPI Synced",
				`Published initial version v${result.version}`,
			);
		}
	} catch {
	} finally {
		syncingOpenApi.value = false;
	}
}

async function syncAllPlatform() {
	await Promise.all([
		canSyncEvents.value ? syncPlatformEvents() : undefined,
		canSyncRoles.value ? syncPlatformRoles() : undefined,
		canSyncOpenApi.value ? syncPlatformOpenApi() : undefined,
	]);
}

// TEMPORARY (docs/spec/audit-redaction.md, Java repo): confirms, then runs the one-off
// sweep of already-stored `aud_logs` rows, and toasts the counts.
function confirmRedactExistingAuditLogs() {
	confirm.require({
		message:
			"Redact passwords and secrets from existing audit rows? This rewrites matching rows in place and cannot be undone.",
		header: "Redact Audit Logs",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Redact",
		acceptClass: "p-button-danger",
		accept: runRedactExistingAuditLogs,
	});
}

async function runRedactExistingAuditLogs() {
	redactingAuditLogs.value = true;
	try {
		const result = await redactExistingAuditLogs();
		toast.success(
			"Audit Logs Redacted",
			`${result.redacted} of ${result.scanned} rows redacted`,
		);
	} catch {
	} finally {
		redactingAuditLogs.value = false;
	}
}

const allDashboardCards = [
	{
		title: "Applications",
		description: "Manage applications in the platform ecosystem",
		route: "/applications",
		icon: "pi pi-th-large",
		bgColor: "bg-indigo",
		iconColor: "text-indigo",
	},
	{
		title: "Clients",
		description: "Manage clients and their configurations",
		route: "/clients",
		icon: "pi pi-building",
		bgColor: "bg-blue",
		iconColor: "text-blue",
	},
	{
		title: "Users",
		description: "Manage platform users and their access",
		route: "/users",
		icon: "pi pi-users",
		bgColor: "bg-green",
		iconColor: "text-green",
	},
	{
		title: "Roles",
		description: "Configure roles and permissions",
		route: "/authorization/roles",
		icon: "pi pi-shield",
		bgColor: "bg-purple",
		iconColor: "text-purple",
	},
	{
		title: "Event Types",
		description: "Define event types and schemas for messaging",
		route: "/event-types",
		icon: "pi pi-bolt",
		bgColor: "bg-amber",
		iconColor: "text-amber",
	},
	{
		title: "Subscriptions",
		description: "Manage event subscriptions and routing",
		route: "/subscriptions",
		icon: "pi pi-bell",
		bgColor: "bg-teal",
		iconColor: "text-teal",
	},
];

/** The quick links this user may open (the route guard's rule). */
const dashboardCards = computed(() =>
	allDashboardCards.filter((card) => canEnterRoute(user.value, card.route)),
);
</script>

<template>
  <div class="dashboard-page">
    <div class="page-header">
      <div>
        <h1 class="page-title">Dashboard</h1>
        <p class="page-subtitle">Welcome back, {{ authStore.displayName }}</p>
      </div>
    </div>

    <!-- A user with no platform role reaches only its profile (Go
         ProfileOnlyWithoutRole); say so instead of an empty dashboard. -->
    <div v-if="hasNoPlatformRole" class="fc-card no-access-card">
      <i class="pi pi-lock no-access-icon"></i>
      <div>
        <h2 class="section-title">No platform access</h2>
        <p class="section-subtitle">
          Your account has no platform access. Only your profile is available.
        </p>
        <RouterLink to="/profile" class="no-access-link">Go to your profile</RouterLink>
      </div>
    </div>

    <!-- Quick actions grid -->
    <div v-if="dashboardCards.length > 0" class="cards-grid">
      <RouterLink
        v-for="card in dashboardCards"
        :key="card.title"
        :to="card.route"
        class="dashboard-card"
      >
        <div class="card-content">
          <div class="card-icon" :class="card.bgColor">
            <i :class="[card.icon, card.iconColor]"></i>
          </div>
          <div class="card-info">
            <h3 class="card-title">{{ card.title }}</h3>
            <p class="card-description">{{ card.description }}</p>
          </div>
        </div>
      </RouterLink>
    </div>

    <!-- Platform sync section -->
    <div v-if="showSync" class="sync-section">
      <div class="section-header">
        <div>
          <h2 class="section-title">Platform Sync</h2>
          <p class="section-subtitle">
            Re-apply code-defined platform definitions (events + roles) without restarting the server.
          </p>
        </div>
        <Button
          label="Sync All"
          icon="pi pi-sync"
          :loading="syncingEvents || syncingRoles || syncingOpenApi"
          @click="syncAllPlatform"
        />
      </div>
      <div class="sync-grid">
        <div v-if="canSyncEvents" class="sync-card">
          <div class="sync-icon bg-amber"><i class="pi pi-bolt text-amber"></i></div>
          <div class="sync-info">
            <h3 class="sync-title">Event Types</h3>
            <p class="sync-description">Platform event-type definitions and schemas.</p>
          </div>
          <Button
            label="Sync"
            icon="pi pi-sync"
            severity="secondary"
            outlined
            :loading="syncingEvents"
            @click="syncPlatformEvents"
          />
        </div>
        <div v-if="canSyncRoles" class="sync-card">
          <div class="sync-icon bg-purple"><i class="pi pi-shield text-purple"></i></div>
          <div class="sync-info">
            <h3 class="sync-title">Roles</h3>
            <p class="sync-description">Code-defined platform roles and their permissions.</p>
          </div>
          <Button
            label="Sync"
            icon="pi pi-sync"
            severity="secondary"
            outlined
            :loading="syncingRoles"
            @click="syncPlatformRoles"
          />
        </div>
        <div v-if="canSyncOpenApi" class="sync-card">
          <div class="sync-icon bg-blue"><i class="pi pi-book text-blue"></i></div>
          <div class="sync-info">
            <h3 class="sync-title">OpenAPI</h3>
            <p class="sync-description">Publish this build's OpenAPI document to the Developer portal.</p>
          </div>
          <Button
            label="Sync"
            icon="pi pi-sync"
            severity="secondary"
            outlined
            :loading="syncingOpenApi"
            @click="syncPlatformOpenApi"
          />
        </div>
        <!-- TEMPORARY (docs/spec/audit-redaction.md in the Java repo, "Temporary:
             redact existing rows from the dashboard"): not part of Sync All — a one-off cleanup
             of already-stored rows, remove this card once the sweep is no
             longer needed. -->
        <div v-if="canRedactAuditLogs" class="sync-card">
          <div class="sync-icon bg-red"><i class="pi pi-lock text-red"></i></div>
          <div class="sync-info">
            <h3 class="sync-title">Audit logs</h3>
            <p class="sync-description">Redact passwords and secrets from existing audit rows.</p>
          </div>
          <Button
            label="Redact"
            icon="pi pi-lock"
            severity="danger"
            outlined
            :loading="redactingAuditLogs"
            @click="confirmRedactExistingAuditLogs"
          />
        </div>
      </div>
    </div>

    <!-- Stats section -->
    <div v-if="canViewStats" class="stats-section">
      <h2 class="section-title">Platform Overview</h2>
      <div class="stats-grid">
        <div class="stat-card">
          <p class="stat-label">Total Clients</p>
          <p class="stat-value">{{ fmt(stats?.totalClients) }}</p>
        </div>
        <div class="stat-card">
          <p class="stat-label">Active Users</p>
          <p class="stat-value">{{ fmt(stats?.activeUsers) }}</p>
        </div>
        <div class="stat-card">
          <p class="stat-label">Roles Defined</p>
          <p class="stat-value">{{ fmt(stats?.rolesDefined) }}</p>
        </div>
      </div>

      <h2 class="section-title section-title-secondary">Message Plane (approx.)</h2>
      <p class="section-subtitle">
        Estimates from the Postgres planner. Updated by autovacuum, so they
        can be a few % stale after a heavy bulk insert. Exact counts at this
        scale would require scanning the full table.
      </p>
      <div class="stats-grid">
        <div class="stat-card">
          <p class="stat-label">Events</p>
          <p class="stat-value">{{ fmtApprox(stats?.eventsApprox) }}</p>
        </div>
        <div class="stat-card">
          <p class="stat-label">Dispatch Jobs</p>
          <p class="stat-value">{{ fmtApprox(stats?.dispatchJobsApprox) }}</p>
        </div>
        <div class="stat-card">
          <p class="stat-label">Audit Logs</p>
          <p class="stat-value">{{ fmtApprox(stats?.auditLogsApprox) }}</p>
        </div>
        <div class="stat-card">
          <p class="stat-label">Login Attempts</p>
          <p class="stat-value">{{ fmtApprox(stats?.loginAttemptsApprox) }}</p>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.dashboard-page {
  max-width: 1400px;
  margin: 0 auto;
}

.page-header {
  margin-bottom: 32px;
}

.page-title {
  font-size: 28px;
  font-weight: 600;
  color: #102a43;
  margin: 0;
}

.page-subtitle {
  color: #627d98;
  margin: 8px 0 0;
  font-size: 15px;
}

.cards-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  gap: 20px;
  margin-bottom: 48px;
}

.dashboard-card {
  background: white;
  border-radius: 12px;
  border: 1px solid #e2e8f0;
  padding: 20px;
  text-decoration: none;
  transition: all 0.2s ease;
}

.dashboard-card:hover {
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.08);
  border-color: #cbd5e1;
  transform: translateY(-2px);
}

.card-content {
  display: flex;
  gap: 16px;
}

.card-icon {
  width: 48px;
  height: 48px;
  border-radius: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.card-icon i {
  font-size: 20px;
}

.card-icon.bg-indigo {
  background: #e0e7ff;
}
.card-icon.bg-indigo .text-indigo {
  color: #4f46e5;
}

.card-icon.bg-blue {
  background: #dbeafe;
}
.card-icon.bg-blue .text-blue {
  color: #2563eb;
}

.card-icon.bg-green {
  background: #dcfce7;
}
.card-icon.bg-green .text-green {
  color: #16a34a;
}

.card-icon.bg-purple {
  background: #f3e8ff;
}
.card-icon.bg-purple .text-purple {
  color: #9333ea;
}

.card-icon.bg-amber {
  background: #fef3c7;
}
.card-icon.bg-amber .text-amber {
  color: #d97706;
}

.card-icon.bg-teal {
  background: #ccfbf1;
}
.card-icon.bg-teal .text-teal {
  color: #0d9488;
}

.sync-icon.bg-red {
  background: #fee2e2;
}
.sync-icon.bg-red .text-red {
  color: #dc2626;
}

.card-info {
  flex: 1;
  min-width: 0;
}

.card-title {
  font-size: 16px;
  font-weight: 600;
  color: #1e293b;
  margin: 0 0 4px;
  transition: color 0.2s ease;
}

.dashboard-card:hover .card-title {
  color: #0967d2;
}

.card-description {
  font-size: 14px;
  color: #64748b;
  margin: 0;
  line-height: 1.4;
}

.sync-section {
  margin-top: 32px;
  margin-bottom: 32px;
}

.section-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 16px;
}

.section-subtitle {
  color: #64748b;
  font-size: 13px;
  margin: 4px 0 0;
}

.sync-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  gap: 16px;
}

.sync-card {
  display: flex;
  align-items: center;
  gap: 12px;
  background: white;
  border: 1px solid #e2e8f0;
  border-radius: 12px;
  padding: 14px 16px;
}

.sync-icon {
  width: 36px;
  height: 36px;
  border-radius: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.sync-icon i {
  font-size: 16px;
}

.sync-info {
  flex: 1;
  min-width: 0;
}

.sync-title {
  font-size: 14px;
  font-weight: 600;
  color: #102a43;
  margin: 0;
}

.sync-description {
  font-size: 12px;
  color: #64748b;
  margin: 2px 0 0;
}

.stats-section {
  margin-top: 48px;
}

.section-title {
  font-size: 18px;
  font-weight: 600;
  color: #243b53;
  margin: 0 0 16px;
}

.section-title-secondary {
  margin-top: 32px;
}

.stats-section .section-subtitle {
  margin: -8px 0 16px;
}

.stats-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 16px;
}

.stat-card {
  background: white;
  border-radius: 12px;
  border: 1px solid #e2e8f0;
  padding: 20px;
}

.stat-label {
  font-size: 14px;
  color: #64748b;
  margin: 0 0 8px;
}

.stat-value {
  font-size: 28px;
  font-weight: 600;
  color: #102a43;
  margin: 0;
}

.no-access-card {
	display: flex;
	gap: 1rem;
	align-items: flex-start;
	padding: 1.25rem;
	margin-bottom: 1.5rem;
}

.no-access-icon {
	font-size: 1.5rem;
	color: var(--text-color-secondary);
}

.no-access-link {
	display: inline-block;
	margin-top: 0.5rem;
}
</style>
