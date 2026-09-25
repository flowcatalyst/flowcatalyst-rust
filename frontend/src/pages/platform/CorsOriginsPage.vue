<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, onMounted } from "vue";
import { corsApi, type CorsOrigin } from "@/api/cors";
import { getErrorMessage } from "@/utils/errors";

const origins = ref<CorsOrigin[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);
const searchQuery = ref("");

// Add origin dialog
const showAddDialog = ref(false);
const newOrigin = ref("");
const newDescription = ref("");
const addLoading = ref(false);
const addError = ref<string | null>(null);

// Delete confirmation dialog
const showDeleteDialog = ref(false);
const originToDelete = ref<CorsOrigin | null>(null);
const deleteLoading = ref(false);

const filteredOrigins = computed(() => {
	if (!searchQuery.value) return origins.value;
	const query = searchQuery.value.toLowerCase();
	return origins.value.filter(
		(origin) =>
			origin.origin.toLowerCase().includes(query) ||
			origin.description?.toLowerCase().includes(query),
	);
});

onMounted(async () => {
	await loadOrigins();
});

async function loadOrigins() {
	loading.value = true;
	error.value = null;
	try {
		const response = await corsApi.list();
		origins.value = response.corsOrigins;
	} catch (e) {
		error.value =
			e instanceof Error ? e.message : "Failed to load CORS origins";
	} finally {
		loading.value = false;
	}
}

function openAddDialog() {
	newOrigin.value = "";
	newDescription.value = "";
	addError.value = null;
	showAddDialog.value = true;
}

async function addOrigin() {
	if (!newOrigin.value.trim()) {
		addError.value = "Origin URL is required";
		return;
	}

	// Basic validation
	const origin = newOrigin.value.trim().toLowerCase();
	if (!origin.startsWith("http://") && !origin.startsWith("https://")) {
		addError.value = "Origin must start with http:// or https://";
		return;
	}

	addLoading.value = true;
	addError.value = null;

	try {
		// POST returns `{ id }` only (CreatedResponse) — re-fetch the list to
		// show the new row with its server-side fields.
		await corsApi.create({
			origin: origin,
			description: newDescription.value.trim() || undefined,
		});
		await loadOrigins();
		showAddDialog.value = false;
		toast.success("Success", `CORS origin "${origin}" added successfully`);
	} catch (e: unknown) {
		addError.value = getErrorMessage(e, "Failed to add CORS origin");
	} finally {
		addLoading.value = false;
	}
}

function confirmDelete(origin: CorsOrigin) {
	originToDelete.value = origin;
	showDeleteDialog.value = true;
}

async function deleteOrigin() {
	if (!originToDelete.value) return;

	deleteLoading.value = true;

	try {
		await corsApi.delete(originToDelete.value.id);
		origins.value = origins.value.filter(
			(o) => o.id !== originToDelete.value?.id,
		);
		showDeleteDialog.value = false;
		toast.success("Success", `CORS origin "${originToDelete.value.origin}" removed`);
	} catch (e: unknown) {
	} finally {
		deleteLoading.value = false;
		originToDelete.value = null;
	}
}

function formatDate(dateString: string) {
	return new Date(dateString).toLocaleDateString();
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">CORS Origins</h1>
        <p class="page-subtitle">
          Manage allowed origins for cross-origin API requests. Origins are cached for 5 minutes.
        </p>
      </div>
      <Button label="Add Origin" icon="pi pi-plus" @click="openAddDialog" />
    </header>

    <Message v-if="error" severity="error" class="error-message">{{ error }}</Message>

    <div class="fc-card">
      <div class="toolbar">
        <IconField class="search-wrapper">
          <InputIcon class="pi pi-search" />
          <InputText v-model="searchQuery" placeholder="Search origins..." />
        </IconField>
      </div>

      <div v-if="loading" class="loading-container">
        <ProgressSpinner strokeWidth="3" />
      </div>

      <DataTable
        v-else
        :value="filteredOrigins"
        paginator
        :rows="10"
        :rowsPerPageOptions="[10, 25, 50]"
        stripedRows
        emptyMessage="No CORS origins configured"
      >
        <Column field="origin" header="Origin" sortable>
          <template #body="{ data }">
            <code class="origin-code">{{ data.origin }}</code>
          </template>
        </Column>
        <Column field="description" header="Description" sortable>
          <template #body="{ data }">
            <span class="description-text">{{ data.description || '—' }}</span>
          </template>
        </Column>
        <Column field="createdAt" header="Added" sortable style="width: 120px">
          <template #body="{ data }">
            {{ formatDate(data.createdAt) }}
          </template>
        </Column>
        <Column header="Actions" style="width: 80px">
          <template #body="{ data }">
            <Button
              icon="pi pi-trash"
              text
              rounded
              severity="danger"
              v-tooltip="'Remove'"
              @click="confirmDelete(data)"
            />
          </template>
        </Column>
      </DataTable>
    </div>

    <!-- Add Origin Dialog -->
    <Dialog
      v-model:visible="showAddDialog"
      header="Add CORS Origin"
      modal
      :style="{ width: '500px' }"
    >
      <div class="dialog-content">
        <Message v-if="addError" severity="error" :closable="false" class="dialog-error">
          {{ addError }}
        </Message>

        <p class="dialog-description">
          Add an origin URL that should be allowed to make cross-origin requests to the API.
        </p>

        <div class="field">
          <label for="origin">Origin URL *</label>
          <InputText
            id="origin"
            v-model="newOrigin"
            placeholder="https://app.example.com"
            class="w-full"
            @keyup.enter="addOrigin"
          />
          <small class="field-help">
            Include protocol (https://) and port if non-standard. No trailing slash or path.
          </small>
        </div>

        <div class="field">
          <label for="description">Description</label>
          <Textarea
            id="description"
            v-model="newDescription"
            placeholder="e.g., Production SPA, Development environment"
            class="w-full"
            rows="2"
          />
        </div>
      </div>

      <template #footer>
        <Button label="Cancel" text @click="showAddDialog = false" :disabled="addLoading" />
        <Button label="Add Origin" icon="pi pi-plus" @click="addOrigin" :loading="addLoading" />
      </template>
    </Dialog>

    <!-- Delete Confirmation Dialog -->
    <Dialog
      v-model:visible="showDeleteDialog"
      header="Remove CORS Origin"
      modal
      :style="{ width: '450px' }"
    >
      <div class="dialog-content">
        <p>Are you sure you want to remove this CORS origin?</p>
        <code class="origin-code-block">{{ originToDelete?.origin }}</code>

        <Message severity="warn" :closable="false" class="warning-message">
          Requests from this origin will be blocked by browsers after the cache expires (up to 5
          minutes).
        </Message>
      </div>

      <template #footer>
        <Button label="Cancel" text @click="showDeleteDialog = false" :disabled="deleteLoading" />
        <Button
          label="Remove"
          icon="pi pi-trash"
          severity="danger"
          @click="deleteOrigin"
          :loading="deleteLoading"
        />
      </template>
    </Dialog>
  </div>
</template>

<style scoped>
.toolbar {
  margin-bottom: 16px;
}

.search-wrapper :deep(.pi-search) {
  color: #94a3b8;
}

.loading-container {
  display: flex;
  justify-content: center;
  padding: 60px;
}

.error-message {
  margin-bottom: 16px;
}

.origin-code {
  background: #f1f5f9;
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 13px;
  font-family: monospace;
}

.origin-code-block {
  display: block;
  background: #f1f5f9;
  padding: 12px 16px;
  border-radius: 6px;
  font-size: 14px;
  font-family: monospace;
  margin: 12px 0;
}

.description-text {
  color: #64748b;
}

.dialog-content {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.dialog-description {
  color: #64748b;
  margin: 0;
}

.dialog-error {
  margin: 0;
}

.warning-message {
  margin: 0;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.field label {
  font-weight: 500;
  color: #334155;
}

.field-help {
  color: #94a3b8;
  font-size: 12px;
}

.w-full {
  width: 100%;
}
</style>
