<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { useRouter } from "vue-router";
import { rolesApi, type Role, type RoleSource } from "@/api/roles";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";

const router = useRouter();

// Read-only drawer — no dirty state, so no discard guard needed.
const { id: roleName, goToList } = useDrawerRoute({
	listPath: "/authorization/roles",
	paramKey: "roleName",
});

const role = ref<Role | null>(null);
const loading = ref(true);
const loadError = ref<string | null>(null);

// Only admin-created roles are editable.
const canEdit = computed(() => role.value?.source === "DATABASE");

// Reactive param: the drawer instance is reused when switching between rows.
watch(
	roleName,
	async (value) => {
		if (!value) return;
		await loadRole(value);
	},
	{ immediate: true },
);

async function loadRole(name: string) {
	loading.value = true;
	loadError.value = null;
	try {
		role.value = await rolesApi.get(name);
	} catch (e) {
		role.value = null;
		loadError.value = e instanceof Error ? e.message : "Failed to load role";
	} finally {
		loading.value = false;
	}
}

function editRole() {
	if (!roleName.value) return;
	// Ordinary route-leave to the full-page editor. Role names contain ":" —
	// encode so the name stays a single path segment.
	void router.push(
		`/authorization/roles/${encodeURIComponent(roleName.value)}/edit`,
	);
}

function getSourceSeverity(source: RoleSource) {
	switch (source) {
		case "CODE":
			return "info";
		case "DATABASE":
			return "success";
		case "SDK":
			return "warn";
		default:
			return "secondary";
	}
}

function getSourceLabel(source: RoleSource) {
	switch (source) {
		case "CODE":
			return "Code-defined";
		case "DATABASE":
			return "Admin-created";
		case "SDK":
			return "SDK-registered";
		default:
			return source;
	}
}

function formatDate(dateStr: string | undefined) {
	if (!dateStr) return "—";
	return new Date(dateStr).toLocaleString();
}

// Parse permission string into parts
function parsePermission(permission: string) {
	const parts = permission.split(":");
	return {
		application: parts[0] || "",
		context: parts[1] || "",
		aggregate: parts[2] || "",
		action: parts[3] || "",
	};
}

function getActionSeverity(action: string) {
	switch (action) {
		case "view":
			return "info";
		case "create":
			return "success";
		case "update":
			return "warn";
		case "delete":
			return "danger";
		default:
			return "secondary";
	}
}
</script>

<template>
  <EntityDrawer
    :title="role?.displayName || role?.shortName || 'Role'"
    :subtitle="role?.name"
    size="wide"
    :loading="loading"
    :error="loadError"
    @close="goToList()"
  >
    <template v-if="role" #header-extra>
      <Tag :value="getSourceLabel(role.source)" :severity="getSourceSeverity(role.source)" />
    </template>

    <template v-if="role">
      <!-- Role Info -->
      <FcFormSection title="Role Details" flat>
        <template v-if="canEdit" #actions>
          <Button icon="pi pi-pencil" label="Edit" text @click="editRole" />
        </template>

        <div class="fc-detail-grid">
          <FcDetailField label="Role Name">
            <code>{{ role.name }}</code>
          </FcDetailField>
          <FcDetailField label="Display Name" :value="role.displayName || '—'" />
          <FcDetailField label="Application">
            <Tag :value="role.applicationCode" severity="secondary" />
          </FcDetailField>
          <FcDetailField label="Source">
            <Tag :value="getSourceLabel(role.source)" :severity="getSourceSeverity(role.source)" />
          </FcDetailField>
          <FcDetailField
            label="Description"
            :value="role.description || 'No description provided'"
            span
          />
          <FcDetailField label="Created" :value="formatDate(role.createdAt)" />
          <FcDetailField label="Updated" :value="formatDate(role.updatedAt)" />
        </div>
      </FcFormSection>

      <!-- Permissions -->
      <FcFormSection :title="`Permissions (${role.permissions?.length || 0})`" flat>
        <DataTable
          v-if="role.permissions && role.permissions.length > 0"
          :value="role.permissions.map((p) => ({ permission: p, ...parsePermission(p) }))"
          :paginator="role.permissions.length > 10"
          :rows="10"
          :rowsPerPageOptions="[10, 25, 50]"
          size="small"
          stripedRows
        >
          <Column field="permission" header="Permission" style="width: 40%">
            <template #body="{ data }">
              <span class="permission-string">{{ data.permission }}</span>
            </template>
          </Column>
          <Column field="application" header="Application" style="width: 15%">
            <template #body="{ data }">
              <Tag :value="data.application" severity="secondary" />
            </template>
          </Column>
          <Column field="context" header="Context" style="width: 15%">
            <template #body="{ data }">
              <span>{{ data.context }}</span>
            </template>
          </Column>
          <Column field="aggregate" header="Aggregate" style="width: 15%">
            <template #body="{ data }">
              <span>{{ data.aggregate }}</span>
            </template>
          </Column>
          <Column field="action" header="Action" style="width: 15%">
            <template #body="{ data }">
              <Tag :value="data.action" :severity="getActionSeverity(data.action)" />
            </template>
          </Column>
        </DataTable>

        <div v-else class="empty-permissions">
          <i class="pi pi-lock"></i>
          <span>This role has no permissions assigned</span>
        </div>
      </FcFormSection>
    </template>
  </EntityDrawer>
</template>

<style scoped>
.permission-string {
  font-family: monospace;
  font-size: 13px;
  color: #475569;
}

.empty-permissions {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 48px;
  color: #64748b;
  gap: 12px;
}

.empty-permissions i {
  font-size: 32px;
  color: #cbd5e1;
}
</style>
