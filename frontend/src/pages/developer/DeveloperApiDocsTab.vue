<script setup lang="ts">
import { nextTick, onMounted, onBeforeUnmount, ref, watch } from "vue";
import "swagger-ui-dist/swagger-ui.css";
// swagger-ui-dist ships its bundle without TS declarations. Runtime API:
// SwaggerUIBundle({ spec, domNode, ... })
// @ts-expect-error — no shipped types
import SwaggerUIBundle from "swagger-ui-dist/swagger-ui-bundle.js";
import { developerApi } from "@/api/developer";
import { ApiError } from "@/api/client";

const props = defineProps<{ applicationId: string }>();

const containerRef = ref<HTMLDivElement | null>(null);
const loading = ref(true);
const noDocs = ref(false);
const error = ref<string | null>(null);
const currentSpec = ref<Record<string, unknown> | null>(null);
const currentVersion = ref<string | null>(null);

function downloadSpec() {
  if (!currentSpec.value) return;
  const info = currentSpec.value["info"] as { title?: string } | undefined;
  const name = (info?.title || props.applicationId)
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  const version = currentVersion.value ? `-${currentVersion.value}` : "";
  const blob = new Blob([JSON.stringify(currentSpec.value, null, 2)], {
    type: "application/json",
  });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `${name || "openapi"}${version}-openapi.json`;
  a.click();
  URL.revokeObjectURL(url);
}

async function mountSpec() {
  loading.value = true;
  noDocs.value = false;
  error.value = null;

  let spec: Record<string, unknown> | null = null;
  currentSpec.value = null;
  currentVersion.value = null;
  try {
    const res = await developerApi.getCurrentOpenApi(props.applicationId);
    spec = res.spec;
    currentSpec.value = res.spec;
    currentVersion.value = res.version ?? null;
  } catch (e) {
    if (e instanceof ApiError && e.status === 404) {
      noDocs.value = true;
    } else {
      error.value = e instanceof Error ? e.message : String(e);
    }
    loading.value = false;
    return;
  }

  // The container is rendered behind a v-show, so the ref is available; but
  // when the applicationId changes we tear down and need a tick before remount.
  await nextTick();
  loading.value = false;
  if (!containerRef.value || !spec) return;
  containerRef.value.innerHTML = "";
  SwaggerUIBundle({
    spec,
    domNode: containerRef.value,
    deepLinking: false,
    // Hide the "Try it out" panel — the developer portal is documentation.
    supportedSubmitMethods: [],
  });
}

onMounted(mountSpec);

watch(
  () => props.applicationId,
  () => {
    if (containerRef.value) containerRef.value.innerHTML = "";
    mountSpec();
  },
);

onBeforeUnmount(() => {
  if (containerRef.value) containerRef.value.innerHTML = "";
});
</script>

<template>
  <div class="api-docs-tab">
    <div v-if="loading" class="loading-container">
      <ProgressSpinner strokeWidth="3" />
    </div>

    <div v-else-if="noDocs" class="fc-card empty-card">
      <i class="pi pi-info-circle"></i>
      <h3>API documentation not available</h3>
      <p>
        This application has not yet published an OpenAPI document. Use the
        SDK's sync flow to upload one, or click <strong>Sync All</strong> on the
        dashboard to publish the platform's own spec.
      </p>
    </div>

    <div v-else-if="error" class="fc-card empty-card error">
      <i class="pi pi-exclamation-triangle"></i>
      <p>{{ error }}</p>
    </div>

    <!-- Swagger UI mounts here. Kept v-show'd (not v-if'd) so the ref is
         always available when mountSpec runs after the network fetch. -->
    <div v-show="!loading && !noDocs && !error" class="docs-toolbar">
      <Button
        label="Download OpenAPI"
        icon="pi pi-download"
        size="small"
        outlined
        @click="downloadSpec"
      />
    </div>
    <div v-show="!loading && !noDocs && !error" ref="containerRef" class="swagger-mount"></div>
  </div>
</template>

<style scoped>
.docs-toolbar {
  display: flex;
  justify-content: flex-end;
  margin-bottom: 8px;
}

.api-docs-tab {
  margin-top: 16px;
}

.loading-container {
  display: flex;
  justify-content: center;
  padding: 64px 0;
}

.empty-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  padding: 64px 24px;
  text-align: center;
  color: var(--text-color-secondary);
  max-width: 560px;
  margin: 32px auto;
}

.empty-card i {
  font-size: 40px;
  color: var(--p-blue-500, #3b82f6);
}

.empty-card.error i {
  color: var(--p-orange-500, #f97316);
}

.empty-card h3 {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
  color: var(--text-color);
}

.empty-card p {
  margin: 0;
  font-size: 14px;
  line-height: 1.6;
}

.swagger-mount {
  background: var(--surface-card, white);
  border-radius: 8px;
  overflow: hidden;
}
</style>
