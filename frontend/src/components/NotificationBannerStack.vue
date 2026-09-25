<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { onNotification, type Notification } from "@/utils/errorBus";

interface ActiveBanner extends Notification {
	id: number;
}

const banners = ref<ActiveBanner[]>([]);
const timers = new Map<number, number>();
let nextId = 1;
let unsubscribe: (() => void) | null = null;

onMounted(() => {
	unsubscribe = onNotification((n: Notification) => {
		const id = nextId++;
		banners.value = [...banners.value, { ...n, id }];
		if (typeof n.life === "number" && n.life > 0) {
			const handle = window.setTimeout(() => dismiss(id), n.life);
			timers.set(id, handle);
		}
	});
});

onUnmounted(() => {
	unsubscribe?.();
	timers.forEach((t) => clearTimeout(t));
	timers.clear();
});

function dismiss(id: number) {
	banners.value = banners.value.filter((b) => b.id !== id);
	const handle = timers.get(id);
	if (handle !== undefined) {
		clearTimeout(handle);
		timers.delete(id);
	}
}

function iconFor(severity: string): string {
	switch (severity) {
		case "success":
			return "pi pi-check-circle";
		case "warn":
			return "pi pi-exclamation-triangle";
		case "error":
			return "pi pi-times-circle";
		default:
			return "pi pi-info-circle";
	}
}
</script>

<template>
  <div v-if="banners.length" class="banner-stack" aria-live="polite">
    <div
      v-for="banner in banners"
      :key="banner.id"
      class="banner"
      :class="`banner-${banner.severity}`"
      role="alert"
    >
      <i :class="iconFor(banner.severity)" class="banner-icon" />
      <div class="banner-body">
        <span class="banner-summary">{{ banner.summary }}</span>
        <span v-if="banner.detail" class="banner-detail">{{ banner.detail }}</span>
      </div>
      <button
        type="button"
        class="banner-dismiss"
        aria-label="Dismiss notification"
        @click="dismiss(banner.id)"
      >
        <i class="pi pi-times" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.banner-stack {
  position: sticky;
  top: 0;
  /* Above PrimeVue's modal layer (drawer masks start at 1101) so toasts stay visible */
  z-index: 1400;
  display: flex;
  flex-direction: column;
  gap: 1px;
  pointer-events: none;
}

.banner {
  pointer-events: auto;
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 10px 16px 10px 14px;
  font-size: 13px;
  line-height: 1.45;
  border-bottom: 1px solid rgba(15, 23, 42, 0.08);
  border-left: 4px solid transparent;
  box-shadow: 0 1px 2px rgba(15, 23, 42, 0.04);
}

.banner-success {
  background: #ecfdf5;
  color: #065f46;
  border-left-color: #10b981;
}
.banner-info {
  background: #eff6ff;
  color: #1e3a8a;
  border-left-color: #3b82f6;
}
.banner-warn {
  background: #fffbeb;
  color: #78350f;
  border-left-color: #f59e0b;
}
.banner-error {
  background: #fef2f2;
  color: #991b1b;
  border-left-color: #ef4444;
}

.banner-icon {
  font-size: 16px;
  flex-shrink: 0;
  margin-top: 2px;
}

.banner-body {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 4px 10px;
}

.banner-summary {
  font-weight: 600;
}

.banner-detail {
  font-weight: 400;
  opacity: 0.92;
  word-break: break-word;
}

.banner-dismiss {
  flex-shrink: 0;
  background: none;
  border: none;
  cursor: pointer;
  padding: 4px 6px;
  margin: -2px -4px 0 0;
  color: currentColor;
  opacity: 0.55;
  border-radius: 4px;
  transition: opacity 0.15s, background-color 0.15s;
  font-size: 12px;
  line-height: 1;
}

.banner-dismiss:hover {
  opacity: 1;
  background-color: rgba(15, 23, 42, 0.05);
}
</style>
