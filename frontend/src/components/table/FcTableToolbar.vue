<script setup lang="ts">
import { onUnmounted, ref } from "vue";
import type Popover from "primevue/popover";

withDefaults(
	defineProps<{
		searchPlaceholder?: string;
		/** Badge on the Filters button — popup filters only */
		activeFilterCount?: number;
		/** Shows Clear All — includes the global search */
		hasActiveFilters?: boolean;
		showRefresh?: boolean;
		/** Hide the quick-search box on views with no free-text filter */
		showSearch?: boolean;
	}>(),
	{
		searchPlaceholder: "Search...",
		activeFilterCount: 0,
		hasActiveFilters: false,
		showRefresh: false,
		showSearch: true,
	},
);

const search = defineModel<string>("search", { default: "" });

const emit = defineEmits<{
	clearAll: [];
	refresh: [];
}>();

const popover = ref<InstanceType<typeof Popover> | null>(null);

function toggleFilters(event: MouseEvent) {
	popover.value?.toggle(event);
}

// Own outside-click dismissal. PrimeVue's built-in one loses a click after a
// nested overlay interaction (its overlay-click bus re-arms `selfClick` and
// never clears it), so with appendTo="self" dropdowns the first outside click
// after picking an option gets swallowed. pointerdown + a contains-check has
// no such state.
function onDocPointerDown(event: PointerEvent) {
	const target = event.target as HTMLElement | null;
	if (
		target?.closest(".fc-filter-popover") ||
		target?.closest(".fc-filters-toggle")
	) {
		return;
	}
	popover.value?.hide();
}

function onPopoverShow() {
	document.addEventListener("pointerdown", onDocPointerDown);
}

function onPopoverHide() {
	document.removeEventListener("pointerdown", onDocPointerDown);
}

onUnmounted(() => {
	document.removeEventListener("pointerdown", onDocPointerDown);
});

defineExpose({
	/** Close the filter popup programmatically */
	hideFilters: () => popover.value?.hide(),
});
</script>

<template>
  <div class="fc-table-toolbar">
    <div class="toolbar-start">
      <IconField v-if="showSearch">
        <InputIcon class="pi pi-search" />
        <InputText
          v-model="search"
          :placeholder="searchPlaceholder"
          class="toolbar-search"
        />
      </IconField>
      <slot name="start" />
    </div>
    <div class="toolbar-end">
      <slot name="actions" />
      <Button
        v-if="showRefresh"
        v-tooltip.bottom="'Refresh'"
        icon="pi pi-refresh"
        text
        severity="secondary"
        @click="emit('refresh')"
      />
      <Button
        v-if="hasActiveFilters"
        label="Clear All"
        icon="pi pi-filter-slash"
        text
        severity="secondary"
        @click="emit('clearAll')"
      />
      <Button
        v-if="$slots['filters']"
        label="Filters"
        icon="pi pi-filter"
        outlined
        class="fc-filters-toggle"
        :badge="activeFilterCount > 0 ? String(activeFilterCount) : undefined"
        @click="toggleFilters"
      />
    </div>

    <!-- Filter popup: pages provide arbitrary filter controls here, bound
         directly to their list state. Dropdowns inside MUST use
         appendTo="self" — panels teleported to body count as outside clicks
         and would dismiss the popover. -->
    <Popover
      ref="popover"
      class="fc-filter-popover"
      @show="onPopoverShow"
      @hide="onPopoverHide"
    >
      <div class="filter-popover-body">
        <slot name="filters" />
      </div>
    </Popover>
  </div>
</template>

<style scoped>
.fc-table-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}

.toolbar-start {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.toolbar-search {
  width: 260px;
  max-width: 100%;
}

.toolbar-end {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

/* Slot content carries this component's scope even after teleport */
.filter-popover-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
</style>

<style>
/* Popover panel chrome — its own elements don't carry this component's scope */
.fc-filter-popover.p-popover {
  width: min(360px, calc(100vw - 32px));
}

.fc-filter-popover .p-popover-content {
  padding: 16px;
}
</style>
