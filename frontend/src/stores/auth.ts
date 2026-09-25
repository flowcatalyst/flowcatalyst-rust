import { defineStore } from "pinia";
import { ref, computed } from "vue";

export interface User {
	id: string;
	email: string;
	name: string;
	clientId: string | null;
	roles: string[];
	/**
	 * The caller's effective permission codes from `/auth/me` (patterns such
	 * as `platform:*:*:*` included; match them with `@/utils/permissions`).
	 * `null` when the backend predates the field: the SPA then falls back to
	 * its old rule, a platform admin role reaches everything.
	 */
	permissions: string[] | null;
	/**
	 * The account signs in through a federated identity provider, which owns
	 * its password: the SPA hides password self-service. Absent means false.
	 */
	ssoManaged?: boolean;
}

export const useAuthStore = defineStore("auth", () => {
	// State
	const user = ref<User | null>(null);
	const isLoading = ref(true);
	const error = ref<string | null>(null);
	const accessibleClients = ref<string[]>([]);
	const selectedClientId = ref<string | null>(null);

	// Computed
	const isAuthenticated = computed(() => user.value !== null);

	const displayName = computed(
		() => user.value?.name || user.value?.email || "Unknown",
	);

	const isPlatformAdmin = computed(() => {
		const roles = user.value?.roles || [];
		return roles.some((r) => r.startsWith("platform:"));
	});

	const isMultiClient = computed(() => accessibleClients.value.length > 1);

	const currentClientId = computed(
		() => selectedClientId.value || user.value?.clientId || null,
	);

	const userInitials = computed(() => {
		const name = displayName.value;
		if (!name || name === "Unknown") return "?";
		const parts = name.split(" ");
		const first = parts[0];
		const last = parts[parts.length - 1];
		if (parts.length >= 2 && first && last) {
			return ((first[0] ?? "") + (last[0] ?? "")).toUpperCase();
		}
		return name.substring(0, 2).toUpperCase();
	});

	// Actions
	function setLoading(loading: boolean) {
		isLoading.value = loading;
	}

	function setError(err: string | null) {
		error.value = err;
		isLoading.value = false;
	}

	function setUser(newUser: User, clients: string[] = []) {
		user.value = newUser;
		accessibleClients.value = clients;
		selectedClientId.value = newUser.clientId;
		isLoading.value = false;
		error.value = null;
	}

	function clearAuth() {
		user.value = null;
		accessibleClients.value = [];
		selectedClientId.value = null;
		isLoading.value = false;
		error.value = null;
	}

	function selectClient(clientId: string) {
		if (accessibleClients.value.includes(clientId)) {
			selectedClientId.value = clientId;
		}
	}

	return {
		// State
		user,
		isLoading,
		error,
		accessibleClients,
		selectedClientId,
		// Computed
		isAuthenticated,
		displayName,
		isPlatformAdmin,
		isMultiClient,
		currentClientId,
		userInitials,
		// Actions
		setLoading,
		setError,
		setUser,
		clearAuth,
		selectClient,
	};
});
