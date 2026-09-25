<script setup lang="ts">
import { useId } from "vue";

withDefaults(
	defineProps<{
		label: string;
		required?: boolean;
		help?: string;
		error?: string;
		/** Span the full width of an .fc-form-grid */
		span?: boolean;
	}>(),
	{
		required: false,
		help: undefined,
		error: undefined,
		span: false,
	},
);

const id = useId();
</script>

<template>
  <div class="fc-form-field" :class="{ 'fc-span-2': span }">
    <label :for="id" class="fc-field-label">
      {{ label }}
      <span v-if="required" class="fc-required">*</span>
    </label>
    <slot :id="id" />
    <small v-if="error" class="fc-field-error">{{ error }}</small>
    <small v-else-if="$slots['help'] || help" class="fc-field-help">
      <slot name="help">{{ help }}</slot>
    </small>
  </div>
</template>

<style scoped>
.fc-form-field {
  min-width: 0;
}
</style>
