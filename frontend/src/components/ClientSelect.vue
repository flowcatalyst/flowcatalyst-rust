<script setup lang="ts">
import { ref, watch, onMounted } from "vue";
import type { FilterOption } from "@/api/filter-options";
import { useClientOptions } from "@/composables/useClientOptions";
import { matchClientOptions } from "@/utils/client-options";

interface Props {
	modelValue: string | null;
	placeholder?: string;
	disabled?: boolean;
	showClear?: boolean;
	invalid?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
	placeholder: "Search for a client...",
	disabled: false,
	showClear: true,
	invalid: false,
});

const emit = defineEmits<{
	"update:modelValue": [value: string | null];
	"client-selected": [client: FilterOption | null];
}>();

// The choices are the active clients the caller can access
// (/bff/filter-options/clients), not the anchor-only client search: a
// client-scoped user picks one of its own clients.
const { options, loading, ensureLoaded } = useClientOptions();
const selectedClient = ref<FilterOption | null>(null);
const suggestions = ref<FilterOption[]>([]);

async function selectById(id: string | null) {
	if (!id) {
		selectedClient.value = null;
		return;
	}
	try {
		await ensureLoaded();
	} catch (e) {
		console.error("Failed to load clients:", e);
	}
	selectedClient.value = options.value.find((o) => o.value === id) ?? { value: id, label: id };
}

onMounted(() => selectById(props.modelValue));

watch(
	() => props.modelValue,
	(newValue) => {
		if (newValue !== (selectedClient.value?.value ?? null)) void selectById(newValue);
	},
);

async function searchClients(event: { query: string }) {
	try {
		await ensureLoaded();
	} catch (e) {
		console.error("Failed to load clients:", e);
	}
	suggestions.value = matchClientOptions(options.value, event.query);
}

function onSelect(event: { value: FilterOption }) {
	selectedClient.value = event.value;
	emit("update:modelValue", event.value.value);
	emit("client-selected", event.value);
}

function onClear() {
	selectedClient.value = null;
	emit("update:modelValue", null);
	emit("client-selected", null);
}
</script>

<template>
  <AutoComplete
    v-model="selectedClient"
    :suggestions="suggestions"
    optionLabel="label"
    :placeholder="placeholder"
    :disabled="disabled"
    :loading="loading"
    :invalid="invalid"
    :dropdown="true"
    dropdownMode="current"
    forceSelection
    class="client-select"
    @complete="searchClients"
    @item-select="onSelect"
    @clear="onClear"
  >
    <template #option="{ option }">
      <div class="client-option">
        <span class="client-name">{{ option.label }}</span>
      </div>
    </template>
    <template #empty>
      <div class="empty-message">No clients found</div>
    </template>
  </AutoComplete>
</template>

<style scoped>
.client-select {
  width: 100%;
}

.client-select :deep(.p-autocomplete-input) {
  width: 100%;
}

.client-option {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 4px 0;
}

.client-name {
  font-weight: 500;
}

.empty-message {
  padding: 8px 12px;
  color: #64748b;
  font-size: 13px;
}
</style>
