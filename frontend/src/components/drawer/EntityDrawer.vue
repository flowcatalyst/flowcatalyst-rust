<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useConfirm } from "primevue/useconfirm";
import { confirmDiscardChanges } from "@/composables/useDrawerRoute";

const props = withDefaults(
	defineProps<{
		title: string;
		subtitle?: string;
		/** two-thirds: a working panel (payload + attempts side by side), not a form. */
		size?: "default" | "wide" | "two-thirds";
		loading?: boolean;
		error?: string | null;
		dirty?: boolean;
	}>(),
	{
		subtitle: undefined,
		size: "default",
		loading: false,
		error: null,
		dirty: false,
	},
);

const emit = defineEmits<{
	/** Fired after the hide animation completes — the host navigates on it. */
	close: [];
}>();

const confirm = useConfirm();

// Controlled visibility: PrimeVue's X / Escape / mask click only *request* a
// close via @update:visible; the drawer stays open unless we commit here.
const visible = ref(false);

// Mount → open, so deep links get the slide-in animation too.
onMounted(() => {
	visible.value = true;
});

async function requestClose(force = false) {
	if (!visible.value) return;
	if (props.dirty && !force) {
		const ok = await confirmDiscardChanges(confirm);
		if (!ok) return;
	}
	visible.value = false;
}

function onVisibleUpdate(value: boolean) {
	if (!value) void requestClose();
}

defineExpose({
	/** Programmatic close; force=true skips the dirty confirm (delete flows). */
	close: requestClose,
});
</script>

<template>
  <!-- Non-modal peek panel: the list stays scrollable and clickable underneath
       (clicking another row switches the drawer via the reactive :id param).
       dismissable stays off — an outside-click listener would close the drawer
       before a row click could navigate. Close = X, Escape, or browser Back. -->
  <Drawer
    :visible="visible"
    position="right"
    :modal="false"
    :block-scroll="false"
    :dismissable="false"
    class="entity-drawer"
    :class="`entity-drawer-${size}`"
    @update:visible="onVisibleUpdate"
    @hide="emit('close')"
  >
    <template #header>
      <div class="entity-drawer-header">
        <div class="entity-drawer-titles">
          <h2 class="entity-drawer-title">{{ title }}</h2>
          <p v-if="subtitle" class="entity-drawer-subtitle">{{ subtitle }}</p>
        </div>
        <div v-if="$slots['header-extra']" class="entity-drawer-header-extra">
          <slot name="header-extra" />
        </div>
      </div>
    </template>

    <div v-if="loading" class="entity-drawer-state">
      <ProgressSpinner style="width: 40px; height: 40px" />
    </div>
    <Message v-else-if="error" severity="error" :closable="false">
      {{ error }}
    </Message>
    <template v-else>
      <slot />
    </template>

    <template v-if="$slots['footer']" #footer>
      <div class="entity-drawer-footer">
        <slot name="footer" />
      </div>
    </template>
  </Drawer>
</template>

<style scoped>
.entity-drawer-header {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
  flex: 1;
}

.entity-drawer-titles {
  min-width: 0;
}

.entity-drawer-title {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
  color: #1e293b;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.entity-drawer-subtitle {
  margin: 2px 0 0;
  font-size: 13px;
  color: #64748b;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.entity-drawer-header-extra {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.entity-drawer-state {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 48px 0;
}

.entity-drawer-footer {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  gap: 8px;
  width: 100%;
}
</style>

<style>
/* Drawer chrome — PrimeVue's own elements don't carry this component's scope.
   Triple-class selectors: the theme's positional width rule (20rem) ties a
   two-class selector on specificity and wins on injection order. */

/* Instant open/close: the theme's 0.5s slide feels sluggish and delays the
   close→navigate handoff. With no animation Vue ends the transition on the
   next frame, so @hide (and navigation) fire immediately. */
.p-drawer-right .p-drawer.entity-drawer.p-drawer-enter-active,
.p-drawer-right .p-drawer.entity-drawer.p-drawer-leave-active {
  animation: none;
}
.p-drawer.entity-drawer.entity-drawer-default {
  width: min(560px, calc(100vw - 24px));
}

.p-drawer.entity-drawer.entity-drawer-wide {
  width: min(800px, calc(100vw - 24px));
}

.p-drawer.entity-drawer.entity-drawer-two-thirds {
  width: min(66.667vw, calc(100vw - 24px));
}

/* No modal mask anymore — a stronger shadow separates the panel from the
   still-active page underneath */
.p-drawer.entity-drawer {
  box-shadow: -8px 0 32px rgba(15, 23, 42, 0.18);
}

.p-drawer.entity-drawer .p-drawer-header {
  /* PrimeVue sizes the header to content; stretch it so the title area
     can actually use the drawer width */
  width: 100%;
  gap: 12px;
}

.p-drawer.entity-drawer .p-drawer-content {
  padding: 20px;
  overflow-y: auto;
  flex: 1;
}

.p-drawer.entity-drawer .p-drawer-footer {
  border-top: 1px solid #e2e8f0;
  padding: 12px 20px;
}
</style>
