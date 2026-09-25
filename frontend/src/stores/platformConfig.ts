import { defineStore } from "pinia";
import { ref, computed } from "vue";

export interface PlatformFeatures {
	messagingEnabled: boolean;
}

export interface PlatformConfig {
	features: PlatformFeatures;
	// Configurable brand name; the SPA uses it for the document title and as the
	// fallback brand. Defaults to "FlowCatalyst".
	platformName: string;
}

const DEFAULT_CONFIG: PlatformConfig = {
	features: {
		messagingEnabled: true,
	},
	platformName: "FlowCatalyst",
};

export const usePlatformConfigStore = defineStore("platformConfig", () => {
	// State
	const config = ref<PlatformConfig>(DEFAULT_CONFIG);
	const isLoaded = ref(false);
	const error = ref<string | null>(null);

	// Computed
	const messagingEnabled = computed(
		() => config.value.features.messagingEnabled,
	);

	const platformName = computed(
		() => config.value.platformName || DEFAULT_CONFIG.platformName,
	);

	// Actions
	async function loadConfig(force = false): Promise<void> {
		if (isLoaded.value && !force) return;

		try {
			const response = await fetch("/api/config/platform");
			if (response.ok) {
				const data = await response.json();
				config.value = { ...DEFAULT_CONFIG, ...data };
				// Reflect the configured brand in the browser tab.
				if (config.value.platformName) {
					document.title = config.value.platformName;
				}
			} else {
				console.warn("Failed to load platform config, using defaults");
			}
		} catch (err) {
			console.warn("Failed to load platform config, using defaults:", err);
			error.value = err instanceof Error ? err.message : "Unknown error";
		} finally {
			isLoaded.value = true;
		}
	}

	function reset() {
		config.value = DEFAULT_CONFIG;
		isLoaded.value = false;
		error.value = null;
	}

	return {
		// State
		config,
		isLoaded,
		error,
		// Computed
		messagingEnabled,
		platformName,
		// Actions
		loadConfig,
		reset,
	};
});
