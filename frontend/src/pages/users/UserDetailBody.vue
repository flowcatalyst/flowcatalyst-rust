<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, watch } from "vue";
import {
	usersApi,
	type User,
	type ClientAccessGrant,
	type RoleAssignment,
	type RolesAssignedResponse,
	type ApplicationAccessGrant,
	type ApplicationAccessAssignedResponse,
	type AvailableApplication,
	type PrincipalScope,
} from "@/api/users";
import { clientsApi, type Client } from "@/api/clients";
import { rolesApi, type Role } from "@/api/roles";
import { getErrorMessage } from "@/utils/errors";
import { passwordPolicyError } from "@/utils/passwordPolicy";
import { useClientOptions } from "@/composables/useClientOptions";
import { useDirtyForm } from "@/composables/useDirtyForm";
import { useConfirm } from "primevue/useconfirm";

const props = defineProps<{
	userId: string;
	/** Open the info card in edit mode once loaded (?edit=true deep link) */
	autoEdit?: boolean;
	/**
	 * Client-administrator hosting (drawer under /client-administration/users).
	 * Bounds the body to what a non-anchor admin may see/do: no scope/client
	 * re-association, no client-access section (its listing is anchor-only on
	 * the backend), no all-applications toggle, no delete. Client labels
	 * resolve via the shared filter-options cache instead of paging the full
	 * client list, and the app picker preselects only grants inside the
	 * admin's available set.
	 */
	clientScoped?: boolean;
}>();

const emit = defineEmits<{
	/** Any successful mutation — hosts use it to refresh the list */
	changed: [];
	/** The user was deleted — hosts close the drawer / navigate away */
	deleted: [];
	/** Initial load finished (null = load failed) */
	loaded: [user: User | null];
}>();

// Whether the main edit form is open — drives which template branch renders.
const editing = ref(false);
/**
 * The host's real dirty flag: true only once the main edit form is open AND
 * has actual unsaved changes (see `dirty` below), not merely for being open.
 * Kept in sync by the watcher after `dirty` is declared.
 */
const dirtyModel = defineModel<boolean>("dirty", { default: false });

const confirm = useConfirm();

// clientScoped label lookups only — the full client list stays unloaded.
const { ensureLoaded: ensureClientOptions, getLabel: getClientOptionLabel } =
	useClientOptions();

const user = ref<User | null>(null);
const clients = ref<Client[]>([]);
const clientGrants = ref<ClientAccessGrant[]>([]);
const loading = ref(true);
const loadFailed = ref(false);
const saving = ref(false);

// Edit form
const editName = ref("");
const editScope = ref<"ANCHOR" | "PARTNER" | "CLIENT" | null>(null);
const editClientId = ref<string | null>(null);

const { dirty, markClean, reset: resetDirty } = useDirtyForm(() => ({
	name: editName.value,
	scope: editScope.value,
	clientId: editClientId.value,
}));

// Propagate real dirty state to the host (see `dirtyModel` above): only
// while actually editing, and only once fields have actually changed.
watch(
	() => editing.value && dirty.value,
	(value) => {
		dirtyModel.value = value;
	},
);

const scopeOptions = [
	{ label: "Anchor", value: "ANCHOR" },
	{ label: "Partner", value: "PARTNER" },
	{ label: "Client", value: "CLIENT" },
];

// Add client access dialog
const showAddClientDialog = ref(false);
const clientSearchQuery = ref("");
const selectedClient = ref<Client | null>(null);
const filteredClients = ref<Client[]>([]);

// Role management
const roleAssignments = ref<RoleAssignment[]>([]);
const availableRoles = ref<Role[]>([]);
const showRolePickerDialog = ref(false);
const roleSearchQuery = ref("");
const selectedRoleNames = ref<Set<string>>(new Set());
const savingRoles = ref(false);

// Application access management
const applicationAccessGrants = ref<ApplicationAccessGrant[]>([]);
const availableApplications = ref<AvailableApplication[]>([]);
const showAppPickerDialog = ref(false);
const appSearchQuery = ref("");
const selectedAppIds = ref<Set<string>>(new Set());
const savingApps = ref(false);
// Whether the user has access to every application (present and future) — the
// application-axis analogue of the anchor client tier. When true the explicit
// grant list is moot and hidden. Only an all-applications admin may turn it on
// (backend-enforced); turning it off is open to any user admin.
const allApplications = ref(false);

// Delete user
const showDeleteDialog = ref(false);
const deleteLoading = ref(false);

// Send password reset email
const showSendResetDialog = ref(false);
const sendingReset = ref(false);

// Reset two-factor (internal-auth users only): clears enrolled factors,
// recovery codes, pending PINs and trusted devices — re-triggers 2FA
// onboarding at next sign-in (lost-device recovery).
const resettingTwoFactor = ref(false);

// Direct password reset (admin sets a new password for the user).
// Used when the user can't receive the email (e.g. lost inbox access).
const showResetPasswordDialog = ref(false);
const resettingPassword = ref(false);
const resetPasswordNew = ref("");
const resetPasswordConfirm = ref("");
const resetPasswordError = ref("");

// Internal-auth users only — OIDC users manage credentials at their IDP.
const canSendPasswordReset = computed(
	() => user.value?.idpType === "INTERNAL" && !!user.value?.email,
);
const canResetPassword = computed(() => user.value?.idpType === "INTERNAL");

const isAnchorUser = computed(() => user.value?.isAnchorUser ?? false);

// Confirmed second factors on the account (from the principal detail read).
// Empty ⇒ no 2FA enrolled — which is the case that gets a self-service
// "Forgot password" diverted to admin approval instead of an email.
const twoFactorMethods = computed(() => user.value?.twoFactorMethods ?? []);
const hasTwoFactor = computed(() => twoFactorMethods.value.length > 0);
const twoFactorSummary = computed(() => {
	if (!hasTwoFactor.value) return "None";
	return twoFactorMethods.value
		.map((m) =>
			m === "TOTP"
				? "Authenticator app"
				: m === "EMAIL_PIN"
					? "Email code"
					: m,
		)
		.join(", ");
});

const userType = computed(() => {
	if (!user.value) return null;

	// Use the explicit scope if available
	if (user.value.scope) {
		switch (user.value.scope) {
			case "ANCHOR":
				return { label: "Anchor", severity: "warn", icon: "pi pi-star" };
			case "PARTNER":
				return { label: "Partner", severity: "info", icon: undefined };
			case "CLIENT":
				return { label: "Client", severity: "secondary", icon: undefined };
		}
	}

	// Fallback to derived logic for backwards compatibility
	if (user.value.isAnchorUser) {
		return { label: "Anchor", severity: "warn", icon: "pi pi-star" };
	}
	const grantedCount = clientGrants.value.length;
	if (grantedCount > 0 || !user.value.clientId) {
		return { label: "Partner", severity: "info", icon: undefined };
	}
	return { label: "Client", severity: "secondary", icon: undefined };
});

const homeClient = computed(() => {
	if (!user.value?.clientId) return null;
	return clients.value.find((c) => c.id === user.value?.clientId);
});

// Home-client display name. Client admins can't page the full client list, so
// clientScoped mode resolves the label from the shared filter-options cache;
// the platform mode keeps the clients-array lookup.
const homeClientName = computed(() => {
	if (!user.value?.clientId) return undefined;
	if (props.clientScoped) return getClientOptionLabel(user.value.clientId);
	return homeClient.value?.name;
});

const grantedClients = computed(() => {
	return clientGrants.value.map((g) => {
		const client = clients.value.find((c) => c.id === g.clientId);
		return {
			...g,
			clientName: client?.name || g.clientId,
			clientIdentifier: client?.identifier || "",
		};
	});
});

const availableClients = computed(() => {
	const existingIds = new Set([
		user.value?.clientId,
		...clientGrants.value.map((g) => g.clientId),
	]);
	return clients.value.filter((c) => !existingIds.has(c.id));
});

// Roles the principal *can* be assigned, gated by the application(s) they
// can actually access. ANCHOR-scope users have implicit access to all apps
// so they see every role. CLIENT/PARTNER users are bounded by
// `applicationAccessGrants` (which is in turn bounded by their client's
// enabled apps) — assigning a role from an inaccessible app silently
// produces no effective permissions because the auth context filters by
// accessible_application_ids. So we hide those roles upstream.
//
// Already-assigned roles stay visible so the user can revoke them even if
// the app access was later removed.
const assignableRoles = computed(() => {
	// "All applications" makes every app accessible, so the explicit-grant
	// filter below must not apply — its grant list may be empty, which used to
	// make every role unselectable for client users with all-apps access.
	if (user.value?.scope === "ANCHOR" || allApplications.value) {
		return availableRoles.value;
	}
	const accessibleCodes = new Set(
		applicationAccessGrants.value.map((g) => g.applicationCode),
	);
	const assignedNames = new Set(roleAssignments.value.map((r) => r.roleName));
	return availableRoles.value.filter(
		(r) => accessibleCodes.has(r.applicationCode) || assignedNames.has(r.name),
	);
});

const hiddenRoleCount = computed(
	() => availableRoles.value.length - assignableRoles.value.length,
);

// Roles filtered by search query for the picker
const filteredAvailableRoles = computed(() => {
	const query = roleSearchQuery.value.toLowerCase();
	return assignableRoles.value.filter(
		(r) =>
			r.name.toLowerCase().includes(query) ||
			r.displayName?.toLowerCase().includes(query),
	);
});

// Check if there are unsaved changes in the role picker
const hasRoleChanges = computed(() => {
	const currentRoles = new Set(roleAssignments.value.map((r) => r.roleName));
	if (currentRoles.size !== selectedRoleNames.value.size) return true;
	for (const role of currentRoles) {
		if (!selectedRoleNames.value.has(role)) return true;
	}
	return false;
});

// Filtered available apps for the picker
const filteredAvailableApps = computed(() => {
	const query = appSearchQuery.value.toLowerCase();
	return availableApplications.value.filter(
		(a) =>
			a.name.toLowerCase().includes(query) ||
			a.code.toLowerCase().includes(query),
	);
});

// Check if there are unsaved changes in the app picker
const hasAppChanges = computed(() => {
	const currentApps = new Set(
		applicationAccessGrants.value.map((a) => a.applicationId),
	);
	if (currentApps.size !== selectedAppIds.value.size) return true;
	for (const appId of currentApps) {
		if (!selectedAppIds.value.has(appId)) return true;
	}
	return false;
});

// Reactive param: the hosting drawer is reused when switching between rows,
// so all per-user state resets and reloads whenever userId changes.
watch(
	() => props.userId,
	async (value) => {
		if (!value) return;
		resetState();
		await Promise.all([
			loadUser(),
			// Client admins cannot page the full client list — labels resolve
			// via the shared filter-options cache instead.
			props.clientScoped
				? ensureClientOptions().catch((error) =>
						console.error("Failed to load client options:", error),
					)
				: loadClients(),
			loadAvailableRoles(),
		]);
		if (user.value) {
			const detailLoads = [loadRoleAssignments(), loadApplicationAccess()];
			// The client-access grant listing is anchor-only on the backend
			// (RequireAnchor), and the section is hidden in clientScoped mode.
			if (!props.clientScoped) detailLoads.push(loadClientGrants());
			await Promise.all(detailLoads);
			// Check if we should start in edit mode
			if (props.autoEdit) {
				startEdit();
			}
		}
		loading.value = false;
		emit("loaded", user.value);
	},
	{ immediate: true },
);

function resetState() {
	loading.value = true;
	loadFailed.value = false;
	user.value = null;
	clientGrants.value = [];
	roleAssignments.value = [];
	applicationAccessGrants.value = [];
	availableApplications.value = [];
	allApplications.value = false;
	editing.value = false;
	resetDirty();
	showAddClientDialog.value = false;
	showRolePickerDialog.value = false;
	showAppPickerDialog.value = false;
	showDeleteDialog.value = false;
	showSendResetDialog.value = false;
	showResetPasswordDialog.value = false;
}

async function loadUser() {
	try {
		user.value = await usersApi.get(props.userId);
		editName.value = user.value.name;
	} catch (error) {
		console.error("Failed to fetch user:", error);
		loadFailed.value = true;
	}
}

async function loadClients() {
	try {
		const allClients: typeof clients.value = [];
		let page = 0;
		const pageSize = 100;
		while (true) {
			const response = await clientsApi.list({ page, pageSize });
			allClients.push(...response.clients);
			if (response.clients.length < pageSize) break;
			page++;
		}
		clients.value = allClients;
	} catch (error) {
		console.error("Failed to fetch clients:", error);
	}
}

async function loadClientGrants() {
	try {
		const response = await usersApi.getClientAccess(props.userId);
		clientGrants.value = response.grants;
	} catch (error) {
		console.error("Failed to fetch client grants:", error);
	}
}

async function loadAvailableRoles() {
	try {
		const response = await rolesApi.list();
		availableRoles.value = response.items;
	} catch (error) {
		console.error("Failed to fetch available roles:", error);
	}
}

async function loadRoleAssignments() {
	try {
		const response = await usersApi.getRoles(props.userId);
		roleAssignments.value = response.roles;
	} catch (error) {
		console.error("Failed to fetch role assignments:", error);
	}
}

async function loadApplicationAccess() {
	try {
		const response = await usersApi.getApplicationAccess(props.userId);
		applicationAccessGrants.value = response.applications;
		allApplications.value = response.allApplications;
	} catch (error) {
		console.error("Failed to fetch application access:", error);
	}
}

// Toggle the "access to all applications" flag. Uses the existing assign
// endpoint, preserving the current explicit grant list so flipping back off
// restores the prior selection. One-way: on failure the switch reverts.
async function onToggleAllApplications(value: boolean) {
	savingApps.value = true;
	try {
		const applicationIds = applicationAccessGrants.value.map(
			(a) => a.applicationId,
		);
		const response = await usersApi.assignApplicationAccess(
			props.userId,
			applicationIds,
			value,
		);
		allApplications.value = response.allApplications;
		applicationAccessGrants.value = response.applications;
		toast.success(
			"Success",
			value
				? "Granted access to all applications"
				: "Restricted to specific applications",
		);
		emit("changed");
	} catch {
		// Revert the switch to its prior state; apiFetch surfaces the error.
		allApplications.value = !value;
	} finally {
		savingApps.value = false;
	}
}

async function loadAvailableApplications() {
	try {
		const response = await usersApi.getAvailableApplications(props.userId);
		availableApplications.value = response.applications;
	} catch (error) {
		console.error("Failed to fetch available applications:", error);
	}
}

function startEdit() {
	editName.value = user.value?.name || "";
	// The wire only ever carries ANCHOR/PARTNER/CLIENT; the generated type
	// is plain string (spec has no enums).
	editScope.value = (user.value?.scope ?? null) as PrincipalScope | null;
	editClientId.value = user.value?.clientId ?? null;
	editing.value = true;
	markClean();
}

function cancelEdit() {
	editName.value = user.value?.name || "";
	// The wire only ever carries ANCHOR/PARTNER/CLIENT; the generated type
	// is plain string (spec has no enums).
	editScope.value = (user.value?.scope ?? null) as PrincipalScope | null;
	editClientId.value = user.value?.clientId ?? null;
	editing.value = false;
	resetDirty();
}

async function saveUser() {
	if (!editName.value.trim()) {
		toast.error("Error", "Name is required");
		return;
	}

	saving.value = true;
	try {
		// 1. Display field (name) via the plain update.
		const updated = await usersApi.update(props.userId, {
			name: editName.value,
		});
		user.value!.name = updated.name;

		// 2. Scope / client association is a separate, anchor-gated change with
		//    explicit intent. Only call it when it actually changed.
		const scopeChanged = editScope.value !== user.value!.scope;
		const clientChanged =
			editClientId.value !== (user.value!.clientId ?? null);
		if (scopeChanged || clientChanged) {
			let assoc: User | null = null;
			if (editScope.value === "ANCHOR") {
				assoc = await usersApi.setClientAssociation(props.userId, "*");
			} else if (editScope.value === "CLIENT" && editClientId.value) {
				assoc = await usersApi.setClientAssociation(
					props.userId,
					editClientId.value,
					"CHANGE_CLIENT",
				);
			} else if (editScope.value === "PARTNER" && editClientId.value) {
				assoc = await usersApi.setClientAssociation(
					props.userId,
					editClientId.value,
					"TO_PARTNER",
				);
			} else if (
				editScope.value === "PARTNER" ||
				editScope.value === "CLIENT"
			) {
				toast.error("Error", "Select a client for CLIENT/PARTNER scope");
				saving.value = false;
				return;
			}
			if (assoc) {
				user.value!.scope = assoc.scope;
				user.value!.clientId = assoc.clientId;
			}
		}

		editing.value = false;
		resetDirty();
		toast.success("Success", "User updated successfully");
		emit("changed");
		emit("loaded", user.value);
	} catch {
		// update errors surface via the global error toast
	} finally {
		saving.value = false;
	}
}

async function toggleUserStatus() {
	if (!user.value) return;

	saving.value = true;
	try {
		if (user.value.active) {
			await usersApi.deactivate(props.userId);
			user.value.active = false;
			toast.success("Success", "User deactivated");
		} else {
			await usersApi.activate(props.userId);
			user.value.active = true;
			toast.success("Success", "User activated");
		}
		emit("changed");
		emit("loaded", user.value);
	} catch {
		// errors surface via the global error toast
	} finally {
		saving.value = false;
	}
}

async function sendPasswordReset() {
	if (!user.value) return;
	sendingReset.value = true;
	try {
		const result = await usersApi.sendPasswordReset(props.userId);
		showSendResetDialog.value = false;
		toast.success("Reset email sent", result.message);
	} catch {
		// errors surface via the global error toast
	} finally {
		sendingReset.value = false;
	}
}

function confirmResetTwoFactor() {
	confirm.require({
		message: `Reset two-factor authentication for "${user.value?.name}"? Enrolled factors, recovery codes and trusted devices are cleared; 2FA onboarding re-triggers at their next sign-in.`,
		header: "Reset Two-Factor",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Reset 2FA",
		acceptClass: "p-button-danger",
		accept: () => void resetTwoFactor(),
	});
}

async function resetTwoFactor() {
	resettingTwoFactor.value = true;
	try {
		const result = await usersApi.resetTwoFactor(props.userId);
		toast.success("2FA reset", result.message);
	} catch {
		// errors surface via the global error toast
	} finally {
		resettingTwoFactor.value = false;
	}
}

function openResetPasswordDialog() {
	resetPasswordNew.value = "";
	resetPasswordConfirm.value = "";
	resetPasswordError.value = "";
	showResetPasswordDialog.value = true;
}

async function resetPasswordDirect() {
	if (!user.value) return;
	resetPasswordError.value = "";
	if (!resetPasswordNew.value) {
		resetPasswordError.value = "Password is required";
		return;
	}
	const policyErr = passwordPolicyError(resetPasswordNew.value, {
		email: user.value.email,
		name: user.value.name,
	});
	if (policyErr) {
		resetPasswordError.value = policyErr;
		return;
	}
	if (resetPasswordNew.value !== resetPasswordConfirm.value) {
		resetPasswordError.value = "Passwords do not match";
		return;
	}
	resettingPassword.value = true;
	try {
		const result = await usersApi.resetPassword(
			props.userId,
			resetPasswordNew.value,
		);
		showResetPasswordDialog.value = false;
		toast.success("Password reset", result.message);
	} catch (e: unknown) {
		resetPasswordError.value = getErrorMessage(e, "Failed to reset password");
	} finally {
		resettingPassword.value = false;
	}
}

async function deleteUser() {
	deleteLoading.value = true;
	try {
		await usersApi.delete(props.userId);
		showDeleteDialog.value = false;
		toast.success("Success", `User "${user.value?.name}" deleted`);
		emit("changed");
		emit("deleted");
	} catch {
		// errors surface via the global error toast
	} finally {
		deleteLoading.value = false;
	}
}

function searchClients(event: { query: string }) {
	const query = event.query.toLowerCase();
	filteredClients.value = availableClients.value.filter(
		(c) =>
			c.name.toLowerCase().includes(query) ||
			c.identifier?.toLowerCase().includes(query),
	);
}

async function grantClientAccess() {
	if (!selectedClient.value) return;

	saving.value = true;
	try {
		const grant = await usersApi.grantClientAccess(
			props.userId,
			selectedClient.value.id,
		);
		clientGrants.value.push(grant);
		showAddClientDialog.value = false;
		selectedClient.value = null;
		clientSearchQuery.value = "";
		toast.success("Success", "Client access granted");
		emit("changed");
	} catch {
		// errors surface via the global error toast
	} finally {
		saving.value = false;
	}
}

async function revokeClientAccess(clientId: string) {
	saving.value = true;
	try {
		await usersApi.revokeClientAccess(props.userId, clientId);
		clientGrants.value = clientGrants.value.filter(
			(g) => g.clientId !== clientId,
		);
		toast.success("Success", "Client access revoked");
		emit("changed");
	} catch {
		// errors surface via the global error toast
	} finally {
		saving.value = false;
	}
}

function openRolePicker() {
	// Initialize selected roles from current assignments
	selectedRoleNames.value = new Set(
		roleAssignments.value.map((r) => r.roleName),
	);
	roleSearchQuery.value = "";
	showRolePickerDialog.value = true;
}

function toggleRole(roleName: string) {
	if (selectedRoleNames.value.has(roleName)) {
		selectedRoleNames.value.delete(roleName);
	} else {
		selectedRoleNames.value.add(roleName);
	}
	// Force reactivity update
	selectedRoleNames.value = new Set(selectedRoleNames.value);
}

function removeSelectedRole(roleName: string) {
	selectedRoleNames.value.delete(roleName);
	selectedRoleNames.value = new Set(selectedRoleNames.value);
}

function cancelRolePicker() {
	showRolePickerDialog.value = false;
}

async function saveRoles() {
	savingRoles.value = true;
	try {
		const roles = Array.from(selectedRoleNames.value);
		const response: RolesAssignedResponse = await usersApi.assignRoles(
			props.userId,
			roles,
		);

		// Update role assignments from response
		roleAssignments.value = response.roles;

		// Update user.roles for display
		if (user.value) {
			user.value.roles = roles;
		}

		showRolePickerDialog.value = false;

		const added = response.added.length;
		const removed = response.removed.length;
		let detail = "Roles updated";
		if (added > 0 && removed > 0) {
			detail = `Added ${added} role(s), removed ${removed} role(s)`;
		} else if (added > 0) {
			detail = `Added ${added} role(s)`;
		} else if (removed > 0) {
			detail = `Removed ${removed} role(s)`;
		}

		toast.success("Success", detail);
		emit("changed");
	} catch {
		// errors surface via the global error toast
	} finally {
		savingRoles.value = false;
	}
}

// Get role display info from available roles
function getRoleDisplay(roleName: string) {
	const role = availableRoles.value.find((r) => r.name === roleName);
	return {
		displayName: role?.displayName || roleName.split(":").pop() || roleName,
		fullName: roleName,
	};
}

// ========== Application Access Functions ==========

async function openAppPicker() {
	if (props.clientScoped) {
		// A client admin's available set is bounded server-side to the client's
		// applications. Always load it, then preselect only grants inside that
		// set — an out-of-reach grant must not ride along in the assignment SET
		// (the backend preserves it automatically).
		await loadAvailableApplications();
		const availIds = new Set(availableApplications.value.map((a) => a.id));
		selectedAppIds.value = new Set(
			applicationAccessGrants.value
				.map((a) => a.applicationId)
				.filter((id) => availIds.has(id)),
		);
	} else {
		// Load available applications if not already loaded
		if (availableApplications.value.length === 0) {
			await loadAvailableApplications();
		}
		// Initialize selected apps from current grants
		selectedAppIds.value = new Set(
			applicationAccessGrants.value.map((a) => a.applicationId),
		);
	}
	appSearchQuery.value = "";
	showAppPickerDialog.value = true;
}

function toggleApp(appId: string) {
	if (selectedAppIds.value.has(appId)) {
		selectedAppIds.value.delete(appId);
	} else {
		selectedAppIds.value.add(appId);
	}
	// Force reactivity update
	selectedAppIds.value = new Set(selectedAppIds.value);
}

function removeSelectedApp(appId: string) {
	selectedAppIds.value.delete(appId);
	selectedAppIds.value = new Set(selectedAppIds.value);
}

function cancelAppPicker() {
	showAppPickerDialog.value = false;
}

async function saveApps() {
	savingApps.value = true;
	try {
		const applicationIds = Array.from(selectedAppIds.value);
		const response: ApplicationAccessAssignedResponse =
			await usersApi.assignApplicationAccess(props.userId, applicationIds);

		// Update application access grants from response
		applicationAccessGrants.value = response.applications;
		allApplications.value = response.allApplications;

		showAppPickerDialog.value = false;

		const added = response.added;
		const removed = response.removed;
		let detail = "Application access updated";
		if (added > 0 && removed > 0) {
			detail = `Added ${added} app(s), removed ${removed} app(s)`;
		} else if (added > 0) {
			detail = `Added ${added} app(s)`;
		} else if (removed > 0) {
			detail = `Removed ${removed} app(s)`;
		}

		toast.success("Success", detail);
		emit("changed");
	} catch {
		// errors surface via the global error toast
	} finally {
		savingApps.value = false;
	}
}

// Get app display info from available applications
function getAppDisplay(appId: string) {
	const app = availableApplications.value.find((a) => a.id === appId);
	return {
		name: app?.name || appId,
		code: app?.code || "",
	};
}

function formatDate(dateStr: string | null | undefined) {
	if (!dateStr) return "—";
	return new Date(dateStr).toLocaleDateString();
}
</script>

<template>
  <div v-if="loading" class="loading-container">
    <ProgressSpinner strokeWidth="3" />
  </div>

  <Message v-else-if="loadFailed" severity="error" :closable="false">
    Failed to load user
  </Message>

  <template v-else-if="user">
    <!-- User Information -->
    <FcFormSection title="User Information" flat>
      <template #actions>
        <Button v-if="!editing" label="Edit" icon="pi pi-pencil" text @click="startEdit" />
        <template v-else>
          <Button v-if="dirty" label="Discard" text @click="cancelEdit" />
          <Button label="Save" icon="pi pi-check" :disabled="!dirty" :loading="saving" @click="saveUser" />
        </template>
      </template>

      <!-- View mode -->
      <div v-if="!editing" class="fc-detail-grid">
        <FcDetailField label="Name" :value="user.name" />
        <FcDetailField label="Email" :value="user.email" />
        <FcDetailField
          label="Authentication"
          :value="user.idpType === 'INTERNAL' ? 'Internal' : user.idpType"
        />
        <FcDetailField label="Type">
          <Tag
            v-if="userType"
            :value="userType.label"
            :severity="userType.severity"
            :icon="userType.icon"
          />
          <span v-else>—</span>
        </FcDetailField>
        <FcDetailField
          v-if="user.scope === 'CLIENT'"
          label="Client"
          :value="homeClientName"
        />
        <FcDetailField label="Created" :value="formatDate(user.createdAt)" />
      </div>

      <!-- Edit mode -->
      <div v-else class="fc-form-grid">
        <FcFormField label="Name" required>
          <template #default="{ id: fieldId }">
            <InputText :id="fieldId" v-model="editName" />
          </template>
        </FcFormField>
        <!-- Scope/client re-association is anchor territory: clientScoped
             hides both controls (edit is name-only). startEdit still seeds
             editScope/editClientId, so saveUser's no-change comparison holds
             and setClientAssociation is never called. -->
        <FcFormField v-if="!clientScoped" label="Type">
          <template #default="{ id: fieldId }">
            <Select
              :id="fieldId"
              v-model="editScope"
              :options="scopeOptions"
              optionLabel="label"
              optionValue="value"
            />
          </template>
        </FcFormField>
        <FcFormField
          v-if="!clientScoped && (editScope === 'CLIENT' || editScope === 'PARTNER')"
          :label="editScope === 'PARTNER' ? 'Client to grant' : 'Client'"
          span
        >
          <template #default="{ id: fieldId }">
            <Select
              :id="fieldId"
              v-model="editClientId"
              :options="clients"
              optionLabel="name"
              optionValue="id"
              :placeholder="editScope === 'PARTNER' ? 'Select a client to grant' : 'Select client'"
              filter
            />
          </template>
        </FcFormField>
      </div>
    </FcFormSection>

    <!-- Client Access (hidden for client admins: the grant listing is
         anchor-only on the backend and cross-client access is out of a
         client admin's remit) -->
    <FcFormSection v-if="!clientScoped" title="Client Access" flat>
      <template v-if="!isAnchorUser" #actions>
        <Button label="Add Client" icon="pi pi-plus" text @click="showAddClientDialog = true" />
      </template>

      <div v-if="isAnchorUser" class="anchor-notice">
        <i class="pi pi-star"></i>
        <span
          >This user has an anchor domain email and automatically has access to all clients.</span
        >
      </div>

      <template v-else>
        <div v-if="homeClient" class="home-client-section">
          <h3 class="section-subtitle">Home Client</h3>
          <div class="client-item home">
            <div class="client-info">
              <span class="client-name">{{ homeClient.name }}</span>
              <span class="client-identifier">{{ homeClient.identifier }}</span>
            </div>
            <Tag value="Home" severity="secondary" />
          </div>
        </div>

        <div v-if="!homeClient && grantedClients.length === 0" class="no-clients-notice">
          <p>This user has no client access configured.</p>
          <Button
            label="Grant Client Access"
            icon="pi pi-plus"
            text
            @click="showAddClientDialog = true"
          />
        </div>

        <div v-if="grantedClients.length > 0" class="granted-clients-section">
          <h3 class="section-subtitle">Granted Access</h3>
          <DataTable :value="grantedClients" size="small">
            <Column field="clientName" header="Client">
              <template #body="{ data }">
                <div class="client-cell">
                  <span class="client-name">{{ data.clientName }}</span>
                  <span class="client-identifier">{{ data.clientIdentifier }}</span>
                </div>
              </template>
            </Column>
            <Column field="grantedAt" header="Granted">
              <template #body="{ data }">
                {{ formatDate(data.grantedAt) }}
              </template>
            </Column>
            <Column header="" style="width: 80px">
              <template #body="{ data }">
                <Button
                  v-tooltip.top="'Revoke access'"
                  icon="pi pi-trash"
                  text
                  rounded
                  severity="danger"
                  @click="revokeClientAccess(data.clientId)"
                />
              </template>
            </Column>
          </DataTable>
        </div>
      </template>
    </FcFormSection>

    <!-- Roles -->
    <FcFormSection title="Roles" flat>
      <template #actions>
        <Button label="Manage Roles" icon="pi pi-pencil" text @click="openRolePicker" />
      </template>

      <div v-if="roleAssignments.length === 0" class="no-roles-notice">
        <p>No roles assigned to this user.</p>
        <Button label="Assign Roles" icon="pi pi-plus" text @click="openRolePicker" />
      </div>

      <DataTable v-else :value="roleAssignments" size="small">
        <Column field="roleName" header="Role">
          <template #body="{ data }">
            <div class="role-cell">
              <span class="role-name">{{ data.roleName.split(':').pop() }}</span>
              <span class="role-full-name">{{ data.roleName }}</span>
            </div>
          </template>
        </Column>
        <Column field="assignmentSource" header="Source">
          <template #body="{ data }">
            <Tag
              :value="data.assignmentSource"
              :severity="data.assignmentSource === 'MANUAL' ? 'info' : 'secondary'"
            />
          </template>
        </Column>
        <Column field="assignedAt" header="Assigned">
          <template #body="{ data }">
            {{ formatDate(data.assignedAt) }}
          </template>
        </Column>
      </DataTable>
    </FcFormSection>

    <!-- Application Access -->
    <FcFormSection title="Application Access" flat>
      <template v-if="!allApplications" #actions>
        <Button label="Manage Applications" icon="pi pi-pencil" text @click="openAppPicker" />
      </template>

      <!-- All-applications toggle: the application-axis analogue of an anchor.
           Hidden for client admins — only an all-applications administrator
           may grant it (backend-enforced). -->
      <div v-if="!clientScoped" class="all-apps-toggle">
        <ToggleSwitch
          v-model="allApplications"
          inputId="allApplications"
          :disabled="savingApps"
          @update:modelValue="onToggleAllApplications"
        />
        <label for="allApplications" class="all-apps-label">
          <span class="all-apps-title">Access to all applications</span>
          <span class="all-apps-hint">
            This user can access every application, present and future.
          </span>
        </label>
      </div>

      <template v-if="!allApplications">
        <div v-if="applicationAccessGrants.length === 0" class="no-apps-notice">
          <p>No application access granted to this user.</p>
          <Button label="Grant Application Access" icon="pi pi-plus" text @click="openAppPicker" />
        </div>

        <DataTable v-else :value="applicationAccessGrants" size="small">
          <Column field="applicationName" header="Application">
            <template #body="{ data }">
              <div class="app-cell">
                <span class="app-name">{{ data.applicationName || data.applicationId }}</span>
                <span class="app-code">{{ data.applicationCode }}</span>
              </div>
            </template>
          </Column>
        </DataTable>
      </template>
    </FcFormSection>

    <!-- Account Actions -->
    <FcFormSection title="Account Actions" flat>
      <div class="action-items">
        <div v-if="canSendPasswordReset" class="action-item">
          <div class="action-info">
            <strong>Send Password Reset</strong>
            <p>Email the user a single-use link to set a new password.</p>
          </div>
          <Button
            label="Send Email"
            icon="pi pi-envelope"
            severity="secondary"
            outlined
            @click="showSendResetDialog = true"
          />
        </div>

        <div v-if="canResetPassword" class="action-item">
          <div class="action-info">
            <strong>Reset Password</strong>
            <p>Set a new password directly (use when the user can't receive email).</p>
          </div>
          <Button
            label="Reset Password"
            icon="pi pi-key"
            severity="secondary"
            outlined
            @click="openResetPasswordDialog"
          />
        </div>

        <div v-if="user.idpType === 'INTERNAL'" class="action-item">
          <div class="action-info">
            <strong>
              Two-Factor Authentication
              <Tag
                :value="twoFactorSummary"
                :severity="hasTwoFactor ? 'success' : 'secondary'"
                class="tfa-status-tag"
              />
            </strong>
            <p v-if="hasTwoFactor">Enrolled factors, recovery codes and trusted devices — reset to clear them and re-trigger 2FA onboarding at next sign-in.</p>
            <p v-else>No second factor enrolled. Without an authenticator app, a self-service "Forgot password" is diverted to admin approval instead of emailing a reset link.</p>
          </div>
          <Button
            label="Reset 2FA"
            icon="pi pi-mobile"
            severity="secondary"
            outlined
            :disabled="!hasTwoFactor"
            :loading="resettingTwoFactor"
            @click="confirmResetTwoFactor"
          />
        </div>

        <div class="action-item">
          <div class="action-info">
            <strong>{{ user.active ? 'Deactivate User' : 'Activate User' }}</strong>
            <p>
              {{
                user.active
                  ? 'Prevent this user from signing in.'
                  : 'Allow this user to sign in again.'
              }}
            </p>
          </div>
          <Button
            :label="user.active ? 'Deactivate' : 'Activate'"
            :icon="user.active ? 'pi pi-ban' : 'pi pi-check'"
            :severity="user.active ? 'danger' : 'success'"
            outlined
            :loading="saving"
            @click="toggleUserStatus"
          />
        </div>

        <div v-if="!clientScoped" class="action-item">
          <div class="action-info">
            <strong>Delete User</strong>
            <p>Permanently remove this user. Cannot be undone.</p>
          </div>
          <Button
            label="Delete"
            icon="pi pi-trash"
            severity="danger"
            outlined
            @click="showDeleteDialog = true"
          />
        </div>
      </div>
    </FcFormSection>
  </template>

  <!-- Add Client Dialog -->
  <Dialog
    v-model:visible="showAddClientDialog"
    header="Grant Client Access"
    :style="{ width: '450px' }"
    :modal="true"
  >
    <div class="dialog-content">
      <label>Search for a client</label>
      <AutoComplete
        v-model="selectedClient"
        :suggestions="filteredClients"
        optionLabel="name"
        placeholder="Type to search..."
        class="w-full"
        dropdown
        @complete="searchClients"
      >
        <template #option="slotProps">
          <div class="client-option">
            <span class="client-name">{{ slotProps.option.name }}</span>
            <span class="client-identifier">{{ slotProps.option.identifier }}</span>
          </div>
        </template>
      </AutoComplete>
    </div>

    <template #footer>
      <Button label="Cancel" text @click="showAddClientDialog = false" />
      <Button
        label="Grant Access"
        icon="pi pi-check"
        :disabled="!selectedClient"
        :loading="saving"
        @click="grantClientAccess"
      />
    </template>
  </Dialog>

  <!-- Role Picker Dialog (Dual-Pane) -->
  <Dialog
    v-model:visible="showRolePickerDialog"
    header="Manage Roles"
    :style="{ width: '700px' }"
    :modal="true"
    :closable="!savingRoles"
  >
    <div class="role-picker">
      <!-- Left Pane: Available Roles -->
      <div class="role-pane available-roles">
        <div class="pane-header">
          <h4>Available Roles</h4>
          <InputText
            v-model="roleSearchQuery"
            placeholder="Filter roles..."
            class="role-filter"
          />
        </div>
        <div class="role-list">
          <div
            v-for="role in filteredAvailableRoles"
            :key="role.name"
            class="role-item"
            :class="{ selected: selectedRoleNames.has(role.name) }"
            @click="toggleRole(role.name)"
          >
            <div class="role-item-content">
              <span class="role-display-name">{{ role.displayName || role.name }}</span>
              <span class="role-name-code">{{ role.name }}</span>
            </div>
            <i v-if="selectedRoleNames.has(role.name)" class="pi pi-check check-icon"></i>
          </div>
          <div v-if="filteredAvailableRoles.length === 0" class="no-results">No roles found</div>
        </div>
        <p v-if="hiddenRoleCount > 0" class="role-pane-hint">
          {{ hiddenRoleCount }} role<span v-if="hiddenRoleCount !== 1">s</span>
          hidden because their application isn't enabled for this user. Add the
          application under <strong>Application Access</strong> to make them
          available here.
        </p>
      </div>

      <!-- Right Pane: Selected Roles -->
      <div class="role-pane selected-roles">
        <div class="pane-header">
          <h4>Selected Roles ({{ selectedRoleNames.size }})</h4>
        </div>
        <div class="role-list">
          <div
            v-for="roleName in selectedRoleNames"
            :key="roleName"
            class="role-item selected-item"
          >
            <div class="role-item-content">
              <span class="role-display-name">{{ getRoleDisplay(roleName).displayName }}</span>
              <span class="role-name-code">{{ roleName }}</span>
            </div>
            <Button
              v-tooltip.top="'Remove'"
              icon="pi pi-times"
              text
              rounded
              severity="danger"
              size="small"
              @click="removeSelectedRole(roleName)"
            />
          </div>
          <div v-if="selectedRoleNames.size === 0" class="no-results">No roles selected</div>
        </div>
      </div>
    </div>

    <template #footer>
      <Button label="Cancel" text :disabled="savingRoles" @click="cancelRolePicker" />
      <Button
        label="Save Roles"
        icon="pi pi-check"
        :disabled="!hasRoleChanges"
        :loading="savingRoles"
        @click="saveRoles"
      />
    </template>
  </Dialog>

  <!-- Application Picker Dialog (Dual-Pane) -->
  <Dialog
    v-model:visible="showAppPickerDialog"
    header="Manage Application Access"
    :style="{ width: '700px' }"
    :modal="true"
    :closable="!savingApps"
  >
    <div class="app-picker">
      <!-- Left Pane: Available Applications -->
      <div class="app-pane available-apps">
        <div class="pane-header">
          <h4>Available Applications</h4>
          <InputText
            v-model="appSearchQuery"
            placeholder="Filter applications..."
            class="app-filter"
          />
        </div>
        <div class="app-list">
          <div
            v-for="app in filteredAvailableApps"
            :key="app.id"
            class="app-item"
            :class="{ selected: selectedAppIds.has(app.id) }"
            @click="toggleApp(app.id)"
          >
            <div class="app-item-content">
              <span class="app-display-name">{{ app.name }}</span>
              <span class="app-name-code">{{ app.code }}</span>
            </div>
            <i v-if="selectedAppIds.has(app.id)" class="pi pi-check check-icon"></i>
          </div>
          <div v-if="filteredAvailableApps.length === 0" class="no-results">
            No applications found
          </div>
        </div>
      </div>

      <!-- Right Pane: Selected Applications -->
      <div class="app-pane selected-apps">
        <div class="pane-header">
          <h4>Selected Applications ({{ selectedAppIds.size }})</h4>
        </div>
        <div class="app-list">
          <div v-for="appId in selectedAppIds" :key="appId" class="app-item selected-item">
            <div class="app-item-content">
              <span class="app-display-name">{{ getAppDisplay(appId).name }}</span>
              <span class="app-name-code">{{ getAppDisplay(appId).code }}</span>
            </div>
            <Button
              v-tooltip.top="'Remove'"
              icon="pi pi-times"
              text
              rounded
              severity="danger"
              size="small"
              @click="removeSelectedApp(appId)"
            />
          </div>
          <div v-if="selectedAppIds.size === 0" class="no-results">No applications selected</div>
        </div>
      </div>
    </div>

    <template #footer>
      <Button label="Cancel" text :disabled="savingApps" @click="cancelAppPicker" />
      <Button
        label="Save Application Access"
        icon="pi pi-check"
        :disabled="!hasAppChanges"
        :loading="savingApps"
        @click="saveApps"
      />
    </template>
  </Dialog>

  <!-- Send Password Reset Confirmation Dialog -->
  <Dialog
    v-model:visible="showSendResetDialog"
    header="Send Password Reset Email"
    modal
    :style="{ width: '480px' }"
  >
    <div class="dialog-content">
      <p>
        Send a password reset email to <strong>{{ user?.name }}</strong>
        (<code>{{ user?.email }}</code>)?
      </p>
      <Message severity="info" :closable="false">
        The user will receive a single-use link valid for 15 minutes. They will set their own
        password — you will not see or handle it.
        Any previously-issued reset tokens for this user will be invalidated.
      </Message>
    </div>

    <template #footer>
      <Button label="Cancel" text :disabled="sendingReset" @click="showSendResetDialog = false" />
      <Button
        label="Send Email"
        icon="pi pi-envelope"
        :loading="sendingReset"
        @click="sendPasswordReset"
      />
    </template>
  </Dialog>

  <!-- Direct Password Reset Dialog -->
  <Dialog
    v-model:visible="showResetPasswordDialog"
    header="Reset Password"
    modal
    :style="{ width: '480px' }"
  >
    <div class="dialog-content">
      <p>
        Set a new password for <strong>{{ user?.name }}</strong><span v-if="user?.email"> (<code>{{ user?.email }}</code>)</span>.
      </p>
      <Message severity="warn" :closable="false">
        The user will need to sign in with this new password immediately. Only use this when the
        user can't receive the password-reset email (e.g. lost inbox access).
      </Message>
      <div class="form-field">
        <label for="new-password">New password</label>
        <!-- Admin sets ANOTHER user's password: new-password stops Chrome
             autofilling the admin's own saved credential here. -->
        <Password
          id="new-password"
          v-model="resetPasswordNew"
          :feedback="false"
          toggleMask
          :inputProps="{ autocomplete: 'new-password' }"
          inputClass="w-full"
          placeholder="At least 8 characters"
          :disabled="resettingPassword"
        />
      </div>
      <div class="form-field">
        <label for="confirm-password">Confirm password</label>
        <Password
          id="confirm-password"
          v-model="resetPasswordConfirm"
          :feedback="false"
          toggleMask
          :inputProps="{ autocomplete: 'new-password' }"
          inputClass="w-full"
          :disabled="resettingPassword"
        />
      </div>
      <Message v-if="resetPasswordError" severity="error" :closable="false">
        {{ resetPasswordError }}
      </Message>
    </div>

    <template #footer>
      <Button
        label="Cancel"
        text
        :disabled="resettingPassword"
        @click="showResetPasswordDialog = false"
      />
      <Button
        label="Set Password"
        icon="pi pi-key"
        :loading="resettingPassword"
        @click="resetPasswordDirect"
      />
    </template>
  </Dialog>

  <!-- Delete User Confirmation Dialog -->
  <Dialog
    v-model:visible="showDeleteDialog"
    header="Delete User"
    modal
    :style="{ width: '450px' }"
  >
    <div class="dialog-content">
      <p>
        Are you sure you want to delete <strong>{{ user?.name }}</strong
        >?
      </p>
      <Message severity="warn" :closable="false">
        This action cannot be undone. The user will be permanently removed.
      </Message>
    </div>

    <template #footer>
      <Button label="Cancel" text :disabled="deleteLoading" @click="showDeleteDialog = false" />
      <Button
        label="Delete"
        icon="pi pi-trash"
        severity="danger"
        :loading="deleteLoading"
        @click="deleteUser"
      />
    </template>
  </Dialog>
</template>

<style scoped>
.loading-container {
  display: flex;
  justify-content: center;
  align-items: center;
  padding: 60px;
}

.anchor-notice {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 16px;
  background: #fffbeb;
  border: 1px solid #fcd34d;
  border-radius: 8px;
  color: #92400e;
}

.anchor-notice i {
  font-size: 20px;
  color: #f59e0b;
}

.section-subtitle {
  font-size: 13px;
  font-weight: 600;
  color: #64748b;
  margin: 0 0 12px 0;
}

.home-client-section {
  margin-bottom: 20px;
}

.client-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px;
  background: #f8fafc;
  border-radius: 6px;
}

.client-item.home {
  border: 1px solid #e2e8f0;
}

.client-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.client-name {
  font-size: 14px;
  font-weight: 500;
  color: #1e293b;
}

.client-identifier {
  font-size: 12px;
  color: #64748b;
  font-family: monospace;
}

.client-cell {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.no-clients-notice,
.no-roles-notice,
.no-apps-notice {
  text-align: center;
  padding: 24px;
  color: #64748b;
}

.all-apps-toggle {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 12px 0 16px 0;
}

.all-apps-label {
  display: flex;
  flex-direction: column;
  gap: 2px;
  cursor: pointer;
}

.all-apps-title {
  font-weight: 600;
  font-size: 14px;
}

.all-apps-hint {
  font-size: 12px;
  color: #64748b;
}

.no-clients-notice p,
.no-roles-notice p,
.no-apps-notice p {
  margin: 0 0 12px 0;
}

.granted-clients-section {
  margin-top: 20px;
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

.tfa-status-tag {
  margin-left: 8px;
  font-size: 11px;
  vertical-align: middle;
}

.action-info p {
  margin: 0;
  font-size: 13px;
  color: #64748b;
}

.dialog-content {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.dialog-content label {
  font-size: 13px;
  font-weight: 500;
  color: #475569;
}

.client-option {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 4px 0;
}

.role-cell {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.role-name {
  font-size: 14px;
  font-weight: 500;
  color: #1e293b;
}

.role-full-name {
  font-size: 12px;
  color: #64748b;
  font-family: monospace;
}

.app-cell {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.app-name {
  font-size: 14px;
  font-weight: 500;
  color: #1e293b;
}

.app-code {
  font-size: 12px;
  color: #64748b;
  font-family: monospace;
}

.role-display-name {
  font-size: 14px;
  font-weight: 500;
  color: #1e293b;
}

.role-name-code {
  font-size: 12px;
  color: #64748b;
  font-family: monospace;
}

.w-full {
  width: 100%;
}

/* PrimeVue Password forwards `inputClass` to the inner <input>, which
 * doesn't carry this file's Vue scope attribute — so the .w-full class
 * silently no-ops there. Deep-select the rendered wrappers instead so the
 * Reset Password dialog's Password fields fill the form-field width.
 * Same trick used in LoginPage.vue + ResetPasswordPage.vue. */
:deep(.p-password) {
  width: 100%;
}
:deep(.p-password-input) {
  width: 100%;
}

/* Dual-pane role picker styles */
.role-picker {
  display: flex;
  gap: 16px;
  min-height: 350px;
}

.role-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  border: 1px solid #e2e8f0;
  border-radius: 8px;
  overflow: hidden;
}

.pane-header {
  padding: 12px;
  background: #f8fafc;
  border-bottom: 1px solid #e2e8f0;
}

.pane-header h4 {
  margin: 0 0 8px 0;
  font-size: 13px;
  font-weight: 600;
  color: #475569;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.selected-roles .pane-header h4 {
  margin-bottom: 0;
}

.role-filter {
  width: 100%;
}

.role-list {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
}

.role-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  border-radius: 6px;
  cursor: pointer;
  transition: background-color 0.15s;
}

.role-item:hover {
  background: #f1f5f9;
}

.role-item.selected {
  background: #eff6ff;
}

.role-item.selected-item {
  background: #f8fafc;
  cursor: default;
}

.role-item.selected-item:hover {
  background: #f1f5f9;
}

.role-item-content {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.role-item-content .role-display-name {
  font-size: 13px;
  font-weight: 500;
  color: #1e293b;
}

.role-item-content .role-name-code {
  font-size: 11px;
  color: #64748b;
  font-family: monospace;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.check-icon {
  color: #3b82f6;
  font-size: 14px;
  flex-shrink: 0;
}

.no-results {
  padding: 20px;
  text-align: center;
  color: #94a3b8;
  font-size: 13px;
}

.role-pane-hint {
  margin: 8px 12px 12px;
  padding: 8px 12px;
  font-size: 12px;
  color: var(--text-color-secondary);
  background: var(--surface-ground);
  border-left: 3px solid var(--p-warning-color, #f59e0b);
  border-radius: 4px;
}

/* Dual-pane app picker styles (mirrors role picker) */
.app-picker {
  display: flex;
  gap: 16px;
  min-height: 350px;
}

.app-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  border: 1px solid #e2e8f0;
  border-radius: 8px;
  overflow: hidden;
}

.app-pane .pane-header {
  padding: 12px;
  background: #f8fafc;
  border-bottom: 1px solid #e2e8f0;
}

.app-pane .pane-header h4 {
  margin: 0 0 8px 0;
  font-size: 13px;
  font-weight: 600;
  color: #475569;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.selected-apps .pane-header h4 {
  margin-bottom: 0;
}

.app-filter {
  width: 100%;
}

.app-list {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
}

.app-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  border-radius: 6px;
  cursor: pointer;
  transition: background-color 0.15s;
}

.app-item:hover {
  background: #f1f5f9;
}

.app-item.selected {
  background: #eff6ff;
}

.app-item.selected-item {
  background: #f8fafc;
  cursor: default;
}

.app-item.selected-item:hover {
  background: #f1f5f9;
}

.app-item-content {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.app-item-content .app-display-name {
  font-size: 13px;
  font-weight: 500;
  color: #1e293b;
}

.app-item-content .app-name-code {
  font-size: 11px;
  color: #64748b;
  font-family: monospace;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.form-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

@media (max-width: 768px) {
  .role-picker,
  .app-picker {
    flex-direction: column;
    min-height: 500px;
  }

  .role-pane,
  .app-pane {
    min-height: 200px;
  }
}
</style>
