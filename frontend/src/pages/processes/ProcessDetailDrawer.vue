<script setup lang="ts">
import { ref, computed, watch, nextTick } from "vue";
import { useRouter } from "vue-router";
import { useConfirm } from "primevue/useconfirm";
import { toast } from "@/utils/errorBus";
import { processesApi } from "@/api/processes";
import type { Process } from "@/api/processes";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";

const emit = defineEmits<{
	changed: [];
}>();

const router = useRouter();
const confirm = useConfirm();

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
// Read-only viewer — nothing to dirty, so no discard guard needed.
const { id, goToList } = useDrawerRoute({ listPath: "/processes" });

const loading = ref(true);
const loadError = ref<string | null>(null);
const process = ref<Process | null>(null);
const showSource = ref(false);
const renderedSvg = ref("");
const renderError = ref("");
const renderTarget = ref<HTMLDivElement | null>(null);

// Reactive param: the drawer instance is reused when switching between rows.
watch(
	id,
	async (value) => {
		if (!value) return;
		// Reset per-entity view state before loading the next row.
		showSource.value = false;
		renderedSvg.value = "";
		renderError.value = "";
		await load(value);
	},
	{ immediate: true },
);

async function load(processId: string) {
	loading.value = true;
	loadError.value = null;
	try {
		process.value = await processesApi.get(processId);
	} catch {
		process.value = null;
		loadError.value = "Process not found";
	} finally {
		loading.value = false;
	}
}

// Watch the ref itself (not just body): every load assigns a fresh object, so
// a row switch re-renders even when two processes share identical source.
watch(
	process,
	async () => {
		if (!process.value) return;
		renderError.value = "";
		renderedSvg.value = "";
		if (!process.value.body.trim()) return;
		if (process.value.diagramType !== "mermaid") {
			renderError.value = `Unsupported diagram type: ${process.value.diagramType}`;
			return;
		}
		try {
			// Lazy-load mermaid only when we actually need to render — keeps it
			// out of the list-page bundle.
			const mermaid = (await import("mermaid")).default;
			mermaid.initialize({ startOnLoad: false, securityLevel: "strict", theme: "default" });
			// The drawer body only mounts once loading flips false — wait for
			// the DOM before mermaid renders/measures the SVG.
			await nextTick();
			const renderId = `mmd-${Date.now()}`;
			const { svg } = await mermaid.render(renderId, process.value.body);
			renderedSvg.value = svg;
			await nextTick();
		} catch (e) {
			renderError.value = (e as Error).message || "Failed to render diagram";
		}
	},
	{ immediate: true },
);

const canEdit = computed(() => process.value?.status === "CURRENT");
const canArchive = computed(() => process.value?.status === "CURRENT");
const canDelete = computed(() => process.value?.status === "ARCHIVED");

function editProcess() {
	// Full-page editor route (carve-out) — an ordinary route leave closes the drawer.
	void router.push(`/processes/${id.value}/edit`);
}

async function archive() {
	if (!process.value) return;
	try {
		await processesApi.archive(process.value.id);
		toast.success("Process archived", process.value.code);
		emit("changed");
		await load(process.value.id);
	} catch (e) {
		toast.error("Failed to archive", (e as Error).message);
	}
}

function confirmDelete() {
	if (!process.value) return;
	confirm.require({
		message: `Delete process ${process.value.code}? This cannot be undone.`,
		header: "Delete Process",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Delete",
		acceptClass: "p-button-danger",
		accept: deleteProcess,
	});
}

async function deleteProcess() {
	if (!process.value) return;
	try {
		await processesApi.delete(process.value.id);
		toast.success("Process deleted", process.value.code);
		emit("changed");
		void drawer.value?.close(true);
	} catch (e) {
		toast.error("Failed to delete", (e as Error).message);
	}
}

function downloadSvg() {
	if (!renderedSvg.value || !process.value) return;
	const blob = new Blob([renderedSvg.value], { type: "image/svg+xml" });
	triggerDownload(blob, `${process.value.code}.svg`);
}

function downloadPng() {
	if (!renderedSvg.value || !process.value) return;
	const svgBlob = new Blob([renderedSvg.value], { type: "image/svg+xml" });
	const url = URL.createObjectURL(svgBlob);
	const img = new Image();
	img.onload = () => {
		const canvas = document.createElement("canvas");
		// Render at 2x for crisper output.
		const scale = 2;
		canvas.width = img.width * scale;
		canvas.height = img.height * scale;
		const ctx = canvas.getContext("2d");
		if (!ctx) {
			URL.revokeObjectURL(url);
			return;
		}
		ctx.fillStyle = "#ffffff";
		ctx.fillRect(0, 0, canvas.width, canvas.height);
		ctx.scale(scale, scale);
		ctx.drawImage(img, 0, 0);
		URL.revokeObjectURL(url);
		canvas.toBlob((blob) => {
			if (blob && process.value) triggerDownload(blob, `${process.value.code}.png`);
		}, "image/png");
	};
	img.onerror = () => {
		URL.revokeObjectURL(url);
		toast.error("Failed to export PNG", "Could not rasterise diagram");
	};
	img.src = url;
}

function triggerDownload(blob: Blob, filename: string) {
	const url = URL.createObjectURL(blob);
	const a = document.createElement("a");
	a.href = url;
	a.download = filename;
	document.body.appendChild(a);
	a.click();
	document.body.removeChild(a);
	URL.revokeObjectURL(url);
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    :title="process?.name || 'Process'"
    :subtitle="process?.code"
    size="wide"
    :loading="loading"
    :error="loadError"
    @close="goToList()"
  >
    <template v-if="process" #header-extra>
      <Tag
        :value="process.status"
        :severity="process.status === 'CURRENT' ? 'success' : 'secondary'"
      />
    </template>

    <template v-if="process">
      <p v-if="process.description" class="description">{{ process.description }}</p>

      <div class="drawer-actions">
        <Button
          label="Download SVG"
          icon="pi pi-download"
          severity="secondary"
          outlined
          :disabled="!renderedSvg"
          @click="downloadSvg"
        />
        <Button
          label="Download PNG"
          icon="pi pi-image"
          severity="secondary"
          outlined
          :disabled="!renderedSvg"
          @click="downloadPng"
        />
        <Button
          v-if="canEdit"
          label="Edit"
          icon="pi pi-pencil"
          @click="editProcess"
        />
        <Button
          v-if="canArchive"
          label="Archive"
          icon="pi pi-inbox"
          severity="warn"
          outlined
          @click="archive"
        />
        <Button
          v-if="canDelete"
          label="Delete"
          icon="pi pi-trash"
          severity="danger"
          outlined
          @click="confirmDelete"
        />
      </div>

      <div class="diagram-toolbar">
        <Tag v-for="t in process.tags" :key="t" :value="t" severity="secondary" />
        <div class="spacer" />
        <Button
          :label="showSource ? 'Hide source' : 'View source'"
          :icon="showSource ? 'pi pi-eye-slash' : 'pi pi-code'"
          text
          severity="secondary"
          @click="showSource = !showSource"
        />
      </div>

      <div v-if="renderError" class="render-error">
        <i class="pi pi-exclamation-triangle"></i>
        <div>
          <strong>Could not render diagram.</strong>
          <pre>{{ renderError }}</pre>
        </div>
      </div>

      <div
        v-else-if="renderedSvg"
        ref="renderTarget"
        class="mermaid-render"
        v-html="renderedSvg"
      />

      <div v-else class="empty-diagram">
        <i class="pi pi-image"></i>
        <span>No diagram body yet. Edit this process to add Mermaid source.</span>
      </div>

      <div v-if="showSource" class="source-block">
        <pre class="source-pre">{{ process.body || '(empty)' }}</pre>
      </div>
    </template>
  </EntityDrawer>
</template>

<style scoped>
.description {
  margin: 0 0 16px;
  color: var(--text-color-secondary);
}

.drawer-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  margin-bottom: 16px;
}

.diagram-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  margin-bottom: 16px;
}

.spacer { flex: 1; }

.mermaid-render {
  display: flex;
  justify-content: center;
  padding: 24px;
  background: var(--surface-ground);
  border-radius: 6px;
  overflow-x: auto;
}

.mermaid-render :deep(svg) {
  max-width: 100%;
  height: auto;
}

.render-error {
  display: flex;
  gap: 12px;
  background: var(--p-yellow-50, #fffbeb);
  color: var(--p-yellow-800, #854d0e);
  border: 1px solid var(--p-yellow-200, #fde68a);
  padding: 16px;
  border-radius: 6px;
}

.render-error i {
  font-size: 20px;
}

.render-error pre {
  margin-top: 8px;
  font-size: 12px;
  white-space: pre-wrap;
}

.empty-diagram {
  text-align: center;
  padding: 48px 24px;
  color: var(--text-color-secondary);
}

.empty-diagram i {
  font-size: 48px;
  display: block;
  margin-bottom: 12px;
  color: var(--surface-border);
}

.source-block {
  margin-top: 16px;
  border-top: 1px solid var(--surface-border);
  padding-top: 16px;
}

.source-pre {
  background: var(--surface-ground);
  padding: 16px;
  border-radius: 6px;
  font-size: 13px;
  overflow-x: auto;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
</style>
