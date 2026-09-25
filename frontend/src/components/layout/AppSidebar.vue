<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import {
	NAVIGATION_CONFIG,
	visibleNavigation,
	type NavItem,
} from "@/config/navigation";
import { useAuthStore } from "@/stores/auth";
import { usePlatformConfigStore } from "@/stores/platformConfig";
import { useAppThemeStore } from "@/stores/appTheme";

defineProps<{
	collapsed: boolean;
}>();

const emit = defineEmits<{
	toggleCollapse: [];
}>();

const route = useRoute();
const platformConfigStore = usePlatformConfigStore();
const appThemeStore = useAppThemeStore();
const authStore = useAuthStore();
const expandedItems = ref<Record<string, boolean>>({});

// Load app theme on mount
onMounted(() => {
	appThemeStore.loadTheme();
});

// Filter navigation based on the user's permissions (items they cannot open
// are hidden) and the platform configuration
const filteredNavigation = computed(() => {
	return visibleNavigation(NAVIGATION_CONFIG, authStore.user).filter((group) => {
		// Hide Messaging group when messaging is disabled
		if (group.label === "Messaging" && !platformConfigStore.messagingEnabled) {
			return false;
		}
		return true;
	});
});

function toggleExpand(itemLabel: string) {
	expandedItems.value = {
		...expandedItems.value,
		[itemLabel]: !expandedItems.value[itemLabel],
	};
}

function isActive(item: NavItem): boolean {
	if (!item.route) return false;
	return route.path === item.route || route.path.startsWith(item.route + "/");
}
</script>

<template>
  <aside class="sidebar" :class="{ collapsed }">
    <!-- Logo Section -->
    <div class="sidebar-header">
      <div
        v-if="!collapsed"
        class="logo-container"
        :style="{ height: `${appThemeStore.logoHeight}px` }"
      >
        <!-- Custom logo URL -->
        <img
          v-if="appThemeStore.logoUrl"
          :src="appThemeStore.logoUrl"
          class="logo-image"
          :style="{ maxHeight: `${appThemeStore.logoHeight}px` }"
          alt="Logo"
        />
        <!-- Custom logo SVG -->
        <div
          v-else-if="appThemeStore.logoSvg"
          class="logo-svg"
          :style="{ height: `${appThemeStore.logoHeight}px` }"
          v-html="appThemeStore.logoSvg"
        />
        <!-- Default logo icon -->
        <div
          v-else
          class="logo-icon"
          :style="{
            width: `${appThemeStore.logoHeight}px`,
            height: `${appThemeStore.logoHeight}px`,
          }"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="1.5"
              d="M13 10V3L4 14h7v7l9-11h-7z"
            />
          </svg>
        </div>
      </div>
      <button class="collapse-btn" @click="emit('toggleCollapse')">
        <i class="pi" :class="collapsed ? 'pi-chevron-right' : 'pi-chevron-left'"></i>
      </button>
    </div>

    <!-- Navigation -->
    <nav class="sidebar-nav">
      <div v-for="group in filteredNavigation" :key="group.label" class="nav-group">
        <span v-if="!collapsed" class="nav-group-label">{{ group.label }}</span>

        <template v-for="item in group.items" :key="item.label">
          <!-- Parent with children -->
          <div v-if="item.children && item.children.length > 0" class="nav-item-wrapper">
            <button
              class="nav-item has-children"
              :class="{ expanded: expandedItems[item.label] }"
              :title="collapsed ? item.label : ''"
              @click="toggleExpand(item.label)"
            >
              <i :class="item.icon"></i>
              <template v-if="!collapsed">
                <span class="nav-label">{{ item.label }}</span>
                <i
                  class="pi expand-icon"
                  :class="expandedItems[item.label] ? 'pi-chevron-down' : 'pi-chevron-right'"
                ></i>
              </template>
            </button>
            <div v-if="!collapsed && expandedItems[item.label]" class="nav-children">
              <RouterLink
                v-for="child in item.children"
                :key="child.label"
                :to="child.route || '#'"
                class="nav-child-item"
                :class="{ active: isActive(child) }"
              >
                <i :class="child.icon"></i>
                <span class="nav-label">{{ child.label }}</span>
              </RouterLink>
            </div>
          </div>

          <!-- Simple nav item -->
          <RouterLink
            v-else
            :to="item.route || '#'"
            class="nav-item"
            :class="{ active: isActive(item) }"
            :title="collapsed ? item.label : ''"
          >
            <i :class="item.icon"></i>
            <span v-if="!collapsed" class="nav-label">{{ item.label }}</span>
          </RouterLink>
        </template>
      </div>
    </nav>

    <!-- Footer -->
    <div class="sidebar-footer">
      <div v-if="!collapsed" class="version-info">
        <span class="version-label">Version</span>
        <span class="version-number">0.0.1</span>
      </div>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  position: fixed;
  left: 0;
  top: 0;
  bottom: 0;
  width: 260px;
  background: linear-gradient(180deg, #102a43 0%, #0a1929 100%);
  display: flex;
  flex-direction: column;
  transition: width 0.3s ease;
  z-index: 1000;
  overflow: hidden;
}

.sidebar.collapsed {
  width: 72px;
}

.sidebar-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.1);
  min-height: 64px;
}

.logo-container {
  display: flex;
  align-items: center;
}

.logo-icon {
  color: #47a3f3;
  flex-shrink: 0;
}

.logo-icon svg {
  width: 100%;
  height: 100%;
}

.logo-image {
  width: auto;
  object-fit: contain;
  flex-shrink: 0;
}

.logo-svg {
  flex-shrink: 0;
  display: flex;
  align-items: center;
}

.logo-svg :deep(svg) {
  height: 100%;
  width: auto;
}

.collapse-btn {
  background: rgba(255, 255, 255, 0.1);
  border: none;
  border-radius: 6px;
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  color: rgba(255, 255, 255, 0.7);
  transition: all 0.2s ease;
  flex-shrink: 0;
}

.collapse-btn:hover {
  background: rgba(255, 255, 255, 0.15);
  color: #ffffff;
}

.sidebar.collapsed .collapse-btn {
  margin: 0 auto;
}

.sidebar-nav {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 16px 0;
}

.nav-group {
  margin-bottom: 24px;
}

.nav-group-label {
  display: block;
  padding: 0 20px;
  margin-bottom: 8px;
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: rgba(255, 255, 255, 0.4);
}

.nav-item-wrapper {
  display: flex;
  flex-direction: column;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 20px;
  color: rgba(255, 255, 255, 0.7);
  text-decoration: none;
  transition: all 0.2s ease;
  cursor: pointer;
  border: none;
  background: none;
  width: 100%;
  text-align: left;
  font-size: 14px;
}

.nav-item i {
  font-size: 18px;
  width: 24px;
  text-align: center;
  flex-shrink: 0;
}

.nav-item:hover {
  background: rgba(255, 255, 255, 0.08);
  color: #ffffff;
}

.nav-item.active {
  background: rgba(71, 163, 243, 0.15);
  color: #47a3f3;
  border-right: 3px solid #47a3f3;
}

.nav-item.has-children {
  position: relative;
}

.nav-item .expand-icon {
  margin-left: auto;
  font-size: 12px;
  transition: transform 0.2s ease;
}

.nav-children {
  padding-left: 12px;
  overflow: hidden;
}

.nav-child-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 20px 8px 32px;
  color: rgba(255, 255, 255, 0.6);
  text-decoration: none;
  transition: all 0.2s ease;
  font-size: 13px;
}

.nav-child-item i {
  font-size: 14px;
  width: 20px;
  text-align: center;
}

.nav-child-item:hover {
  color: rgba(255, 255, 255, 0.9);
  background: rgba(255, 255, 255, 0.05);
}

.nav-child-item.active {
  color: #47a3f3;
  background: rgba(71, 163, 243, 0.1);
}

.nav-label {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.sidebar.collapsed .nav-item {
  justify-content: center;
  padding: 12px;
}

.sidebar.collapsed .nav-group-label {
  display: none;
}

.sidebar-footer {
  padding: 16px;
  border-top: 1px solid rgba(255, 255, 255, 0.1);
}

.version-info {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.4);
}

.version-number {
  color: rgba(255, 255, 255, 0.6);
}

/* Scrollbar styling */
.sidebar-nav::-webkit-scrollbar {
  width: 4px;
}

.sidebar-nav::-webkit-scrollbar-track {
  background: transparent;
}

.sidebar-nav::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.2);
  border-radius: 2px;
}

.sidebar-nav::-webkit-scrollbar-thumb:hover {
  background: rgba(255, 255, 255, 0.3);
}

@media (max-width: 768px) {
  .sidebar {
    transform: translateX(-100%);
  }

  .sidebar.mobile-open {
    transform: translateX(0);
  }
}
</style>
