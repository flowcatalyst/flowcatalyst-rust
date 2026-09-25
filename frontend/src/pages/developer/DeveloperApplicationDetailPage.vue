<script setup lang="ts">
import { defineAsyncComponent, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
  developerApi,
  type DeveloperApplicationSummary,
} from "@/api/developer";

const route = useRoute();
const router = useRouter();

const appId = ref<string>(String(route.params["id"]));
const app = ref<DeveloperApplicationSummary | null>(null);
const loading = ref(true);
const activeTab = ref<string>(
  typeof route.query["tab"] === "string" ? route.query["tab"] : "api-docs",
);

// Lazy-loaded so non-developers (and users on the Event Types tab) don't pay
// the ~1.4 MB Swagger UI chunk.
const ApiDocsTab = defineAsyncComponent(
  () => import("./DeveloperApiDocsTab.vue"),
);
const EventTypesTab = defineAsyncComponent(
  () => import("./DeveloperEventTypesTab.vue"),
);
const ProcessesTab = defineAsyncComponent(
  () => import("./DeveloperProcessesTab.vue"),
);

async function load() {
  loading.value = true;
  try {
    app.value = await developerApi.getApplication(appId.value);
  } finally {
    loading.value = false;
  }
}

watch(
  () => route.params["id"],
  (v) => {
    appId.value = String(v);
    load();
  },
);

watch(activeTab, (tab) => {
  router.replace({ query: { ...route.query, tab } });
});

onMounted(load);
</script>

<template>
  <div class="page-container">
    <div v-if="loading" class="loading-container">
      <ProgressSpinner strokeWidth="3" />
    </div>

    <template v-else-if="app">
      <header class="page-header">
        <div class="header-content">
          <Button
            icon="pi pi-arrow-left"
            text
            severity="secondary"
            @click="router.push('/developer')"
            v-tooltip="'Back to applications'"
          />
          <div class="header-text">
            <h1 class="page-title">{{ app.name }}</h1>
            <p class="page-subtitle">
              <span class="app-code">{{ app.code }}</span>
              <span v-if="app.description"> · {{ app.description }}</span>
            </p>
          </div>
          <Tag
            v-if="app.currentVersion"
            :value="`API v${app.currentVersion}`"
            severity="info"
          />
        </div>
        <div class="header-actions">
          <Button
            label="Versions"
            icon="pi pi-history"
            severity="secondary"
            outlined
            @click="router.push(`/developer/applications/${appId}/versions`)"
          />
        </div>
      </header>

      <Tabs v-model:value="activeTab" class="developer-tabs">
        <TabList>
          <Tab value="api-docs"><i class="pi pi-book"></i>&nbsp;API Docs</Tab>
          <Tab value="event-types"><i class="pi pi-bolt"></i>&nbsp;Event Types</Tab>
          <Tab value="processes"><i class="pi pi-sitemap"></i>&nbsp;Processes</Tab>
        </TabList>
        <TabPanels>
          <TabPanel value="api-docs">
            <ApiDocsTab :application-id="appId" />
          </TabPanel>
          <TabPanel value="event-types">
            <EventTypesTab :application-id="appId" />
          </TabPanel>
          <TabPanel value="processes">
            <ProcessesTab :application-code="app.code" />
          </TabPanel>
        </TabPanels>
      </Tabs>
    </template>
  </div>
</template>

<style scoped>
.loading-container {
  display: flex;
  justify-content: center;
  padding: 64px 0;
}

.page-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 16px;
  margin-bottom: 24px;
}

.header-content {
  display: flex;
  align-items: center;
  gap: 12px;
  flex: 1;
  min-width: 0;
}

.header-text {
  flex: 1;
  min-width: 0;
}

.app-code {
  font-family: ui-monospace, SFMono-Regular, monospace;
  color: var(--text-color-secondary);
}

.header-actions {
  display: flex;
  gap: 8px;
  align-items: center;
}

.developer-tabs :deep(.p-tabpanels) {
  padding: 0;
}
</style>
