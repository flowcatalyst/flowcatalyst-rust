<script setup lang="ts">
import { ref, onMounted, computed } from "vue";
import { useRoute, useRouter } from "vue-router";
import { toast } from "@/utils/errorBus";
import { processesApi } from "@/api/processes";
import { canEditProcess } from "@/utils/process-rules";

const route = useRoute();
const router = useRouter();
const processId = computed(() => route.params["id"] as string);

const code = ref("");
const name = ref("");
const description = ref("");
const body = ref("");
const tagsText = ref("");
const loading = ref(true);
const saving = ref(false);

onMounted(async () => {
	try {
		const p = await processesApi.get(processId.value);
		// An archived process is read-only (a stale link included): back to
		// its drawer, which has no Edit for it.
		if (!canEditProcess(p)) {
			toast.error("Read-only", "Archived processes cannot be edited.");
			router.push(`/processes/${processId.value}`);
			return;
		}
		code.value = p.code;
		name.value = p.name;
		description.value = p.description ?? "";
		body.value = p.body;
		tagsText.value = p.tags.join(", ");
	} catch (e) {
		toast.error("Failed to load", (e as Error).message);
		router.push("/processes");
	} finally {
		loading.value = false;
	}
});

async function save() {
	saving.value = true;
	try {
		const tags = tagsText.value
			.split(",")
			.map((t) => t.trim())
			.filter(Boolean);
		await processesApi.update(processId.value, {
			name: name.value.trim(),
			description: description.value.trim(),
			body: body.value,
			tags,
		});
		toast.success("Process updated", code.value);
		router.push(`/processes/${processId.value}`);
	} catch (e) {
		toast.error("Failed to save", (e as Error).message);
	} finally {
		saving.value = false;
	}
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <Button
          label="Back"
          icon="pi pi-arrow-left"
          text
          severity="secondary"
          @click="router.push(`/processes/${processId}`)"
        />
        <h1 class="page-title">Edit process</h1>
        <p class="page-subtitle">
          <span class="font-mono">{{ code }}</span>
        </p>
      </div>
    </header>

    <div v-if="loading" class="loading-container">
      <ProgressSpinner strokeWidth="3" />
    </div>

    <div v-else class="fc-card">
      <div class="form-grid">
        <div class="form-field full-row">
          <label>Name</label>
          <InputText v-model="name" class="full-width" />
        </div>
        <div class="form-field full-row">
          <label>Description</label>
          <Textarea v-model="description" :autoResize="true" rows="2" class="full-width" />
        </div>
        <div class="form-field full-row">
          <label>Tags</label>
          <InputText v-model="tagsText" placeholder="fulfilment, billing" class="full-width" />
          <small>Comma-separated.</small>
        </div>
        <div class="form-field full-row">
          <label>Mermaid source</label>
          <Textarea v-model="body" rows="16" class="full-width source-input" />
        </div>
      </div>

      <div class="form-actions">
        <Button
          label="Cancel"
          severity="secondary"
          text
          @click="router.push(`/processes/${processId}`)"
        />
        <Button label="Save" icon="pi pi-check" :loading="saving" @click="save" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.font-mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }

.loading-container {
  display: flex;
  justify-content: center;
  align-items: center;
  padding: 60px;
}

.form-grid {
  display: grid;
  grid-template-columns: 1fr;
  gap: 16px;
  padding: 8px 0;
}

.form-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.form-field label {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-color-secondary);
}

.form-field small {
  color: var(--text-color-secondary);
  font-size: 12px;
}

.full-row {
  grid-column: 1 / -1;
}

.full-width {
  width: 100%;
}

.source-input :deep(textarea) {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 13px;
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
  border-top: 1px solid var(--surface-border);
  padding-top: 16px;
}
</style>
