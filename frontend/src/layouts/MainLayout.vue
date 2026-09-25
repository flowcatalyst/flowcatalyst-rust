<script setup lang="ts">
import { ref, watch } from "vue";
import { useRoute } from "vue-router";
import { useLocalState } from "@/composables/useLocalState";

const sidebarCollapsed = useLocalState("sidebar-collapsed", false);
const mobileNavOpen = ref(false);
const route = useRoute();

function toggleSidebar() {
	sidebarCollapsed.value = !sidebarCollapsed.value;
}

// Any navigation closes the mobile nav overlay.
watch(
	() => route.fullPath,
	() => {
		mobileNavOpen.value = false;
	},
);
</script>

<template>
  <div class="layout-container">
    <AppSidebar
      :collapsed="sidebarCollapsed && !mobileNavOpen"
      :mobile-open="mobileNavOpen"
      @toggle-collapse="toggleSidebar"
    />
    <div v-if="mobileNavOpen" class="mobile-backdrop" @click="mobileNavOpen = false"></div>
    <div class="layout-main" :class="{ 'sidebar-collapsed': sidebarCollapsed }">
      <div class="mobile-topbar">
        <button class="menu-toggle" @click="mobileNavOpen = true">
          <i class="pi pi-bars"></i>
        </button>
      </div>
      <main class="layout-content">
        <RouterView />
      </main>
    </div>
  </div>
</template>

<style scoped>
.layout-container {
  display: flex;
  min-height: 100vh;
  background-color: #f8fafc;
}

.layout-main {
  flex: 1;
  margin-left: 260px;
  transition: margin-left 0.3s ease;
  display: flex;
  flex-direction: column;
  min-height: 100vh;
}

.layout-main.sidebar-collapsed {
  margin-left: 72px;
}

.layout-content {
  flex: 1;
  padding: 24px;
}

.mobile-topbar {
  display: none;
}

.mobile-backdrop {
  display: none;
}

.menu-toggle {
  background: none;
  border: none;
  padding: 8px;
  cursor: pointer;
  color: #64748b;
  border-radius: 6px;
  transition: all 0.2s ease;
}

.menu-toggle:hover {
  background: #f1f5f9;
  color: #334155;
}

.menu-toggle i {
  font-size: 20px;
}

@media (max-width: 768px) {
  .layout-main {
    margin-left: 0;
  }

  .layout-main.sidebar-collapsed {
    margin-left: 0;
  }

  .mobile-topbar {
    display: flex;
    align-items: center;
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: 48px;
    padding: 0 12px;
    background: #ffffff;
    border-bottom: 1px solid #e2e8f0;
    z-index: 998;
  }

  .mobile-backdrop {
    display: block;
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    z-index: 999;
  }

  .layout-content {
    margin-top: 48px;
    padding: 16px;
  }
}
</style>
