<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useAuthStore } from "@/stores/auth";
import { eventTypesApi } from "@/api/event-types";
import { rolesApi } from "@/api/roles";
import { developerApi } from "@/api/developer";
import { dashboardApi, type DashboardStats } from "@/api/dashboard";
import { toast } from "@/utils/errorBus";

const authStore = useAuthStore();

const syncingEvents = ref(false);
const syncingRoles = ref(false);
const syncingOpenApi = ref(false);

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

onMounted(loadStats);

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
		syncPlatformEvents(),
		syncPlatformRoles(),
		syncPlatformOpenApi(),
	]);
}

const dashboardCards = [
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
		route: "/roles",
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
</script>

<template>
  <div class="dashboard-page">
    <div class="page-header">
      <div>
        <h1 class="page-title">Dashboard</h1>
        <p class="page-subtitle">Welcome back, {{ authStore.displayName }}</p>
      </div>
    </div>

    <!-- Quick actions grid -->
    <div class="cards-grid">
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
    <div class="sync-section">
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
        <div class="sync-card">
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
        <div class="sync-card">
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
        <div class="sync-card">
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
      </div>
    </div>

    <!-- Stats section -->
    <div class="stats-section">
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
</style>
