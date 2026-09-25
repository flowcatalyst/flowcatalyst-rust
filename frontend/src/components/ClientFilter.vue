<script setup lang="ts">
import { onMounted } from "vue";
import { useClientOptions } from "@/composables/useClientOptions";

interface Props {
	modelValue: string | string[] | null;
	multiple?: boolean;
	placeholder?: string;
	showClear?: boolean;
	maxSelectedLabels?: number;
	/** Pass "self" when hosted inside a Popover (e.g. the filter popup) */
	appendTo?: string;
}

withDefaults(defineProps<Props>(), {
	multiple: true,
	placeholder: "All Clients",
	showClear: true,
	maxSelectedLabels: 2,
	appendTo: "body",
});

const emit = defineEmits<{
	"update:modelValue": [value: string | string[] | null];
	change: [];
}>();

const { options, loading, ensureLoaded } = useClientOptions();

onMounted(() => {
	ensureLoaded();
});

function onUpdate(value: string | string[] | null) {
	emit("update:modelValue", value);
	emit("change");
}
</script>

<template>
  <MultiSelect
    v-if="multiple"
    :modelValue="(modelValue as string[])"
    :options="options"
    optionLabel="label"
    optionValue="value"
    :placeholder="placeholder"
    :maxSelectedLabels="maxSelectedLabels"
    :loading="loading"
    :appendTo="appendTo"
    filter
    class="client-filter"
    @update:modelValue="onUpdate"
  />
  <Select
    v-else
    :modelValue="(modelValue as string | null)"
    :options="options"
    optionLabel="label"
    optionValue="value"
    :placeholder="placeholder"
    :showClear="showClear"
    :loading="loading"
    :appendTo="appendTo"
    filter
    class="client-filter"
    @update:modelValue="onUpdate"
  />
</template>

<style scoped>
.client-filter {
  width: 100%;
}
</style>
