<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { rolesApi, type Role } from "@/api/roles";
import { permissionsApi, type Permission } from "@/api/permissions";

const route = useRoute();
const router = useRouter();

const role = ref<Role | null>(null);
const allPermissions = ref<Permission[]>([]);
const loading = ref(true);
const saving = ref(false);
const error = ref<string | null>(null);

// Form state
const displayName = ref("");
const description = ref("");
const selectedPermissions = ref<Set<string>>(new Set());
const clientManaged = ref(false);

const roleName = computed(() => route.params['roleName'] as string);

const hasChanges = computed(() => {
	if (!role.value) return false;

	const permissionsChanged =
		selectedPermissions.value.size !== role.value.permissions.length ||
		!role.value.permissions.every((p) => selectedPermissions.value.has(p));

	return (
		displayName.value !== (role.value.displayName || "") ||
		description.value !== (role.value.description || "") ||
		clientManaged.value !== role.value.clientManaged ||
		permissionsChanged
	);
});

// Group permissions by application for easier selection
const permissionsByApplication = computed(() => {
	const groups = new Map<string, Permission[]>();
	for (const perm of allPermissions.value) {
		const list = groups.get(perm.application) || [];
		list.push(perm);
		groups.set(perm.application, list);
	}
	return groups;
});

const applications = computed(() =>
	Array.from(permissionsByApplication.value.keys()).sort(),
);

// Custom permission entry. Platform permissions are code-defined, so this is
// only offered for non-platform applications: a non-platform app has no
// permission catalogue of its own — its permissions exist only as the strings
// attached to its roles — so "creating a permission" means defining a new
// one here and saving it onto the role. It then appears in the application's
// catalogue for future roles.
const isPlatformRole = computed(() => role.value?.applicationCode === "platform");
const newPerm = ref({ context: "", aggregate: "", action: "" });

const newPermString = computed(() => {
	const app = role.value?.applicationCode ?? "";
	const { context, aggregate, action } = newPerm.value;
	return `${app}:${context.trim()}:${aggregate.trim()}:${action.trim()}`;
});

// Each segment must be a non-empty lowercase token (no colons/spaces) so the
// result is a well-formed 4-segment permission the backend can parse.
const segmentPattern = /^[a-z0-9-]+$/;

// Coerce natural input ("Approve Invoice") into a valid segment on blur,
// rather than leaving Add silently disabled with no explanation.
function slugifySegment(value: string): string {
	return value
		.toLowerCase()
		.replace(/\s+/g, "-")
		.replace(/[^a-z0-9-]/g, "")
		.replace(/-+/g, "-")
		.replace(/^-|-$/g, "");
}
const canAddPermission = computed(() =>
	[newPerm.value.context, newPerm.value.aggregate, newPerm.value.action].every(
		(s) => segmentPattern.test(s.trim()),
	),
);
const permissionExists = computed(() =>
	allPermissions.value.some((p) => p.permission === newPermString.value),
);

onMounted(async () => {
	// Load the role first so we know which application's permissions to fetch —
	// a non-platform role must offer its own application's permissions, not the
	// platform catalogue.
	await loadRole();
	await loadPermissions();
});

async function loadRole() {
	try {
		role.value = await rolesApi.get(roleName.value);

		// Populate form
		displayName.value = role.value.displayName || "";
		description.value = role.value.description || "";
		clientManaged.value = role.value.clientManaged;
		selectedPermissions.value = new Set(role.value.permissions);

		// Redirect if not editable. Return so the rest of the page setup
		// (loadPermissions etc.) doesn't run and flash behind the redirect.
		if (role.value.source !== "DATABASE") {
			toast.warn("Not Editable", "Only admin-created roles can be edited");
			await router.push(
				`/authorization/roles/${encodeURIComponent(roleName.value)}`,
			);
			role.value = null;
			return;
		}
	} catch (e) {
		error.value = e instanceof Error ? e.message : "Failed to load role";
	}
}

async function loadPermissions() {
	if (!role.value) {
		loading.value = false;
		return;
	}
	try {
		const response = await permissionsApi.list(role.value.applicationCode);
		allPermissions.value = response.items;
	} catch (e) {
		// An empty permission catalogue with no explanation looked like a
		// bug; surface the load failure instead.
		error.value =
			e instanceof Error ? e.message : "Failed to load permissions";
	} finally {
		loading.value = false;
	}
}

function togglePermission(permissionString: string) {
	if (selectedPermissions.value.has(permissionString)) {
		selectedPermissions.value.delete(permissionString);
	} else {
		selectedPermissions.value.add(permissionString);
	}
	// Trigger reactivity
	selectedPermissions.value = new Set(selectedPermissions.value);
}

function addCustomPermission() {
	if (!role.value || !canAddPermission.value || permissionExists.value) return;
	const perm = newPermString.value;
	// Surface it in the catalogue list so it renders & groups, then select it.
	// It persists to the role on save (and to the app's catalogue thereafter).
	allPermissions.value = [
		...allPermissions.value,
		{
			permission: perm,
			application: role.value.applicationCode,
			context: newPerm.value.context.trim(),
			aggregate: newPerm.value.aggregate.trim(),
			action: newPerm.value.action.trim(),
			description: "",
		},
	];
	selectedPermissions.value.add(perm);
	selectedPermissions.value = new Set(selectedPermissions.value);
	newPerm.value = { context: "", aggregate: "", action: "" };
}

function selectAllInApplication(application: string) {
	const perms = permissionsByApplication.value.get(application) || [];
	for (const perm of perms) {
		selectedPermissions.value.add(perm.permission);
	}
	selectedPermissions.value = new Set(selectedPermissions.value);
}

function deselectAllInApplication(application: string) {
	const perms = permissionsByApplication.value.get(application) || [];
	for (const perm of perms) {
		selectedPermissions.value.delete(perm.permission);
	}
	selectedPermissions.value = new Set(selectedPermissions.value);
}

function isApplicationFullySelected(application: string): boolean {
	const perms = permissionsByApplication.value.get(application) || [];
	return perms.every((p) => selectedPermissions.value.has(p.permission));
}

function isApplicationPartiallySelected(application: string): boolean {
	const perms = permissionsByApplication.value.get(application) || [];
	const selected = perms.filter((p) =>
		selectedPermissions.value.has(p.permission),
	);
	return selected.length > 0 && selected.length < perms.length;
}

async function saveRole() {
	if (!role.value || saving.value) return;

	saving.value = true;
	try {
		await rolesApi.update(roleName.value, {
			displayName: displayName.value || undefined,
			description: description.value || undefined,
			permissions: Array.from(selectedPermissions.value),
			clientManaged: clientManaged.value,
		});

		toast.success("Saved", "Role updated successfully");

		router.push(`/authorization/roles/${encodeURIComponent(roleName.value)}`);
	} catch {
	} finally {
		saving.value = false;
	}
}

function cancel() {
	router.push(`/authorization/roles/${encodeURIComponent(roleName.value)}`);
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
  <div class="page-container">
    <header class="page-header">
      <div class="header-left">
        <Button
          icon="pi pi-arrow-left"
          text
          rounded
          severity="secondary"
          @click="cancel"
          v-tooltip.right="'Back to role'"
        />
        <div>
          <h1 class="page-title">Edit Role</h1>
          <p class="page-subtitle">{{ role?.name }}</p>
        </div>
      </div>
      <div class="header-right">
        <Button label="Cancel" severity="secondary" text @click="cancel" />
        <Button
          label="Save Changes"
          icon="pi pi-check"
          :loading="saving"
          :disabled="!hasChanges"
          @click="saveRole"
        />
      </div>
    </header>

    <div v-if="loading" class="loading-container">
      <ProgressSpinner strokeWidth="3" />
    </div>

    <div v-else-if="error" class="error-container">
      <i class="pi pi-exclamation-triangle"></i>
      <span>{{ error }}</span>
      <Button label="Go Back" @click="cancel" />
    </div>

    <template v-else-if="role">
      <!-- Basic Info Card -->
      <div class="fc-card">
        <h2 class="card-title">Basic Information</h2>

        <div class="form-grid">
          <div class="form-field">
            <label for="displayName">Display Name</label>
            <InputText
              id="displayName"
              v-model="displayName"
              placeholder="e.g., Tenant Administrator"
              class="w-full"
            />
          </div>

          <div class="form-field">
            <label for="description">Description</label>
            <Textarea
              id="description"
              v-model="description"
              placeholder="Describe what this role grants access to..."
              rows="3"
              class="w-full"
            />
          </div>

          <div class="form-field checkbox-field">
            <Checkbox id="clientManaged" v-model="clientManaged" :binary="true" />
            <label for="clientManaged">
              Client Managed
              <span class="field-hint">Sync this role to client-managed identity providers</span>
            </label>
          </div>
        </div>
      </div>

      <!-- Permissions Card -->
      <div class="fc-card permissions-card">
        <div class="card-header">
          <h2 class="card-title">Permissions</h2>
          <span class="permission-count">{{ selectedPermissions.size }} selected</span>
        </div>

        <!-- Define a new permission for this application (non-platform only;
             platform permissions are code-defined). -->
        <div v-if="!isPlatformRole" class="add-permission">
          <label class="add-permission-label">New permission</label>
          <div class="add-permission-row">
            <Tag :value="role.applicationCode" severity="secondary" />
            <span class="separator">:</span>
            <InputText
              v-model="newPerm.context"
              placeholder="context"
              class="seg-input"
              @blur="newPerm.context = slugifySegment(newPerm.context)"
            />
            <span class="separator">:</span>
            <InputText
              v-model="newPerm.aggregate"
              placeholder="aggregate"
              class="seg-input"
              @blur="newPerm.aggregate = slugifySegment(newPerm.aggregate)"
            />
            <span class="separator">:</span>
            <InputText
              v-model="newPerm.action"
              placeholder="action"
              class="seg-input"
              @blur="newPerm.action = slugifySegment(newPerm.action)"
            />
            <Button
              label="Add"
              icon="pi pi-plus"
              size="small"
              :disabled="!canAddPermission || permissionExists"
              @click="addCustomPermission"
            />
          </div>
          <small v-if="permissionExists" class="add-permission-hint warn">
            <code>{{ newPermString }}</code> already exists
          </small>
          <small v-else class="add-permission-hint">
            Auto-formatted to lowercase letters, numbers and hyphens. Saved onto this role.
          </small>
        </div>

        <div v-if="allPermissions.length === 0" class="empty-permissions">
          <i class="pi pi-lock"></i>
          <span>No permissions available</span>
        </div>

        <div v-else class="permissions-sections">
          <div v-for="application in applications" :key="application" class="application-section">
            <div class="application-header">
              <div class="application-title">
                <Checkbox
                  :modelValue="isApplicationFullySelected(application)"
                  :indeterminate="isApplicationPartiallySelected(application)"
                  :binary="true"
                  @update:modelValue="
                    (val: boolean) =>
                      val
                        ? selectAllInApplication(application)
                        : deselectAllInApplication(application)
                  "
                />
                <Tag :value="application" severity="secondary" />
                <span class="application-count">
                  {{
                    permissionsByApplication
                      .get(application)
                      ?.filter((p) => selectedPermissions.has(p.permission)).length || 0
                  }}
                  / {{ permissionsByApplication.get(application)?.length || 0 }}
                </span>
              </div>
            </div>

            <div class="permissions-grid">
              <div
                v-for="perm in permissionsByApplication.get(application)"
                :key="perm.permission"
                class="permission-item"
                :class="{ selected: selectedPermissions.has(perm.permission) }"
                @click="togglePermission(perm.permission)"
              >
                <Checkbox
                  :modelValue="selectedPermissions.has(perm.permission)"
                  :binary="true"
                  @click.stop
                  @update:modelValue="togglePermission(perm.permission)"
                />
                <div class="permission-info">
                  <div class="permission-name">
                    <span class="context">{{ perm.context }}</span>
                    <span class="separator">:</span>
                    <span class="aggregate">{{ perm.aggregate }}</span>
                    <Tag
                      :value="perm.action"
                      :severity="getActionSeverity(perm.action)"
                      class="action-tag"
                    />
                  </div>
                  <div v-if="perm.description" class="permission-description">
                    {{ perm.description }}
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.header-left {
  display: flex;
  align-items: center;
  gap: 12px;
}

.header-right {
  display: flex;
  align-items: center;
  gap: 12px;
}

.loading-container,
.error-container {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 60px;
  gap: 16px;
  color: #64748b;
}

.error-container i {
  font-size: 48px;
  color: #ef4444;
}

.fc-card {
  margin-bottom: 24px;
}

.card-title {
  font-size: 16px;
  font-weight: 600;
  color: #1e293b;
  margin: 0 0 20px 0;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 20px;
}

.card-header .card-title {
  margin: 0;
}

.permission-count {
  font-size: 13px;
  color: #64748b;
  background: #f1f5f9;
  padding: 4px 12px;
  border-radius: 12px;
}

.form-grid {
  display: flex;
  flex-direction: column;
  gap: 20px;
}

.form-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.form-field label {
  font-size: 13px;
  font-weight: 500;
  color: #475569;
}

.checkbox-field {
  flex-direction: row;
  align-items: flex-start;
  gap: 10px;
}

.checkbox-field label {
  display: flex;
  flex-direction: column;
  gap: 2px;
  cursor: pointer;
}

.field-hint {
  font-size: 12px;
  font-weight: 400;
  color: #64748b;
}

.permissions-card {
  padding-bottom: 0;
}

.add-permission {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 16px;
  margin-bottom: 16px;
  border: 1px dashed #cbd5e1;
  border-radius: 8px;
  background: #f8fafc;
}

.add-permission-label {
  font-size: 13px;
  font-weight: 600;
  color: #475569;
}

.add-permission-row {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.add-permission-row .separator {
  color: #94a3b8;
  font-family: monospace;
}

.seg-input {
  width: 140px;
  font-family: monospace;
}

.add-permission-hint {
  font-size: 12px;
  color: #64748b;
}

.add-permission-hint.warn {
  color: #b45309;
}

.add-permission-hint code {
  font-family: monospace;
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

.permissions-sections {
  display: flex;
  flex-direction: column;
}

.application-section {
  border-top: 1px solid #e2e8f0;
  padding: 16px 0;
}

.application-section:first-child {
  border-top: none;
  padding-top: 0;
}

.application-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 12px;
}

.application-title {
  display: flex;
  align-items: center;
  gap: 10px;
}

.application-count {
  font-size: 12px;
  color: #64748b;
}

.permissions-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(350px, 1fr));
  gap: 8px;
  padding-left: 28px;
}

.permission-item {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 10px 12px;
  border-radius: 6px;
  border: 1px solid #e2e8f0;
  cursor: pointer;
  transition: all 0.15s;
}

.permission-item:hover {
  border-color: #cbd5e1;
  background: #f8fafc;
}

.permission-item.selected {
  border-color: #3b82f6;
  background: #eff6ff;
}

.permission-info {
  flex: 1;
  min-width: 0;
}

.permission-name {
  display: flex;
  align-items: center;
  gap: 2px;
  font-size: 13px;
  font-family: monospace;
}

.permission-name .context {
  color: #64748b;
}

.permission-name .separator {
  color: #94a3b8;
}

.permission-name .aggregate {
  color: #1e293b;
  font-weight: 500;
}

.action-tag {
  margin-left: 8px;
  font-size: 11px;
}

.permission-description {
  font-size: 12px;
  color: #64748b;
  margin-top: 4px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.w-full {
  width: 100%;
}

@media (max-width: 768px) {
  .permissions-grid {
    grid-template-columns: 1fr;
  }
}
</style>
