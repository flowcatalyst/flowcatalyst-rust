<script setup lang="ts">
import { computed } from "vue";

const props = withDefaults(
	defineProps<{
		label: string;
		value?: string | number | null;
		/** Span the full width of an .fc-detail-grid */
		span?: boolean;
	}>(),
	{
		value: undefined,
		span: false,
	},
);

const displayValue = computed(() => {
	if (props.value === null || props.value === undefined || props.value === "") {
		return "—";
	}
	return props.value;
});
</script>

<template>
  <div class="fc-detail-field" :class="{ 'fc-span-2': span }">
    <span class="fc-field-label">{{ label }}</span>
    <div class="fc-detail-value">
      <slot>{{ displayValue }}</slot>
    </div>
  </div>
</template>

<style scoped>
.fc-detail-field {
  min-width: 0;
}
</style>
