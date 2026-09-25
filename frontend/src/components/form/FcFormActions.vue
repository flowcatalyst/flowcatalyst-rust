<script setup lang="ts">
withDefaults(
	defineProps<{
		/** Top border + spacing for inline page use; false inside drawer footers */
		bordered?: boolean;
	}>(),
	{
		bordered: true,
	},
);
</script>

<template>
  <div class="fc-form-actions" :class="{ bordered }">
    <slot />
  </div>
</template>

<style scoped>
.fc-form-actions {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  gap: 8px;
}

.fc-form-actions.bordered {
  border-top: 1px solid #e2e8f0;
  padding-top: 16px;
  margin-top: 20px;
}
</style>
