<script setup lang="ts">
withDefaults(
	defineProps<{
		title?: string;
		description?: string;
		/** Header + divider without card chrome — for drawer bodies */
		flat?: boolean;
	}>(),
	{
		title: undefined,
		description: undefined,
		flat: false,
	},
);
</script>

<template>
  <section
    class="fc-form-section"
    :class="flat ? 'fc-form-section-flat' : 'fc-card'"
  >
    <header
      v-if="title || description || $slots['actions']"
      class="section-header"
    >
      <div class="section-titles">
        <h3 v-if="title" class="section-title">{{ title }}</h3>
        <p v-if="description" class="section-description">{{ description }}</p>
      </div>
      <div v-if="$slots['actions']" class="section-actions">
        <slot name="actions" />
      </div>
    </header>
    <div class="section-body">
      <slot />
    </div>
  </section>
</template>

<style scoped>
.fc-form-section {
  margin-bottom: 16px;
}

.fc-form-section:last-child {
  margin-bottom: 0;
}

.fc-form-section-flat {
  padding-bottom: 20px;
  border-bottom: 1px solid #e2e8f0;
}

.fc-form-section-flat:last-child {
  border-bottom: none;
  padding-bottom: 0;
}

.section-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 12px;
  margin-bottom: 16px;
  padding-bottom: 12px;
  border-bottom: 1px solid #e2e8f0;
}

.fc-form-section-flat .section-header {
  border-bottom: none;
  padding-bottom: 0;
  margin-bottom: 12px;
}

.section-title {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  color: #1e293b;
}

.section-description {
  margin: 4px 0 0;
  font-size: 13px;
  color: #64748b;
}

.section-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.section-body {
  container-type: inline-size;
}
</style>
