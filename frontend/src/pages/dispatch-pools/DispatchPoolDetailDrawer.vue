<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { computed, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { useConfirm } from "primevue/useconfirm";
import { dispatchPoolsApi, type DispatchPool } from "@/api/dispatch-pools";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";
import { useDirtyForm } from "@/composables/useDirtyForm";

const emit = defineEmits<{
	changed: [];
}>();

const route = useRoute();
const confirm = useConfirm();

const editing = ref(false);

// Edit form
const editName = ref("");
const editDescription = ref("");
const editRateLimit = ref<number | null>(null);
const editConcurrency = ref<number | null>(null);

const { dirty, markClean, reset: resetDirty } = useDirtyForm(() => ({
	name: editName.value,
	description: editDescription.value,
	rateLimit: editRateLimit.value,
	concurrency: editConcurrency.value,
}));

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { id, goToList } = useDrawerRoute({
	listPath: "/dispatch-pools",
	dirty: computed(() => editing.value && dirty.value),
});

const loading = ref(true);
const loadError = ref<string | null>(null);
const pool = ref<DispatchPool | null>(null);
const saving = ref(false);

// Reactive param: the drawer instance is reused when switching between rows.
watch(
	id,
	async (value) => {
		if (!value) return;
		editing.value = false;
		resetDirty();
		await loadPool(value);
		if (route.query["edit"] === "true") {
			startEditing();
		}
	},
	{ immediate: true },
);

async function loadPool(poolId: string) {
	loading.value = true;
	loadError.value = null;
	try {
		pool.value = await dispatchPoolsApi.get(poolId);
	} catch {
		pool.value = null;
		loadError.value = "Dispatch pool not found";
	} finally {
		loading.value = false;
	}
}

function startEditing() {
	// Archived pools are read-only (list edit shortcuts may still carry ?edit=true).
	if (pool.value && pool.value.status !== "ARCHIVED") {
		editName.value = pool.value.name;
		editDescription.value = pool.value.description || "";
		editRateLimit.value = pool.value.rateLimit ?? null;
		editConcurrency.value = pool.value.concurrency;
		editing.value = true;
		markClean();
	}
}

function cancelEditing() {
	editing.value = false;
	resetDirty();
}

async function saveChanges() {
	if (!pool.value) return;

	saving.value = true;
	const poolId = pool.value.id;
	try {
		await dispatchPoolsApi.update(poolId, {
			name: editName.value,
			description: editDescription.value || undefined,
			rateLimit: editRateLimit.value || undefined,
			concurrency: editConcurrency.value || undefined,
		});
		await loadPool(poolId);
		editing.value = false;
		resetDirty();
		toast.success("Success", "Pool updated");
		emit("changed");
	} catch {
		// update errors surface via the global error toast
	} finally {
		saving.value = false;
	}
}

function confirmActivate() {
	confirm.require({
		message: "Activate this dispatch pool?",
		header: "Activate Pool",
		icon: "pi pi-check-circle",
		acceptLabel: "Activate",
		accept: activatePool,
	});
}

async function activatePool() {
	if (!pool.value) return;
	try {
		await dispatchPoolsApi.activate(pool.value.id);
		pool.value = await dispatchPoolsApi.get(pool.value.id);
		toast.success("Success", "Pool activated");
		emit("changed");
	} catch {
		// errors surface via the global error toast
	}
}

function confirmSuspend() {
	confirm.require({
		message: "Suspend this dispatch pool? Jobs will not be processed.",
		header: "Suspend Pool",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Suspend",
		acceptClass: "p-button-warning",
		accept: suspendPool,
	});
}

async function suspendPool() {
	if (!pool.value) return;
	try {
		await dispatchPoolsApi.suspend(pool.value.id);
		pool.value = await dispatchPoolsApi.get(pool.value.id);
		toast.success("Success", "Pool suspended");
		emit("changed");
	} catch {
		// errors surface via the global error toast
	}
}

function confirmDelete() {
	confirm.require({
		message: "Delete this dispatch pool? This permanently deletes it and cannot be undone.",
		header: "Delete Pool",
		icon: "pi pi-exclamation-triangle",
		acceptLabel: "Delete",
		acceptClass: "p-button-danger",
		accept: deletePool,
	});
}

async function deletePool() {
	if (!pool.value) return;
	try {
		await dispatchPoolsApi.delete(pool.value.id);
		toast.success("Success", "Pool deleted");
		emit("changed");
		editing.value = false;
		void drawer.value?.close(true);
	} catch {
		// errors surface via the global error toast
	}
}

// Wire status is plain string (spec carries no enum); default covers unknowns.
function getStatusSeverity(status: string) {
	switch (status) {
		case "ACTIVE":
			return "success";
		case "SUSPENDED":
			return "warn";
		case "ARCHIVED":
			return "secondary";
		default:
			return "secondary";
	}
}

function formatDate(dateString: string) {
	return new Date(dateString).toLocaleString();
}

function getScopeLabel(p: DispatchPool) {
	if (p.clientIdentifier) {
		return p.clientIdentifier;
	}
	return "Anchor-level (no client)";
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    :title="pool?.name || 'Dispatch Pool'"
    :subtitle="pool?.code"
    :loading="loading"
    :error="loadError"
    :dirty="editing && dirty"
    @close="goToList()"
  >
    <template v-if="pool" #header-extra>
      <Tag :value="pool.status" :severity="getStatusSeverity(pool.status)" />
    </template>

    <template v-if="pool">
      <!-- Details -->
      <FcFormSection title="Pool Details" flat>
        <template v-if="!editing && pool.status !== 'ARCHIVED'" #actions>
          <Button icon="pi pi-pencil" label="Edit" text @click="startEditing" />
        </template>

        <template v-if="editing">
          <div class="fc-form-grid">
            <FcFormField label="Name" span>
              <template #default="{ id: fieldId }">
                <InputText :id="fieldId" v-model="editName" />
              </template>
            </FcFormField>
            <FcFormField label="Description" span>
              <template #default="{ id: fieldId }">
                <Textarea :id="fieldId" v-model="editDescription" rows="3" />
              </template>
            </FcFormField>
            <FcFormField
              label="Rate Limit (per minute)"
              help="Leave blank to run on concurrency only."
            >
              <template #default="{ id: fieldId }">
                <InputNumber
                  :inputId="fieldId"
                  v-model="editRateLimit"
                  :min="1"
                  placeholder="Unlimited"
                />
              </template>
            </FcFormField>
            <FcFormField label="Concurrency">
              <template #default="{ id: fieldId }">
                <InputNumber :inputId="fieldId" v-model="editConcurrency" :min="1" />
              </template>
            </FcFormField>
          </div>
        </template>

        <template v-else>
          <div class="fc-detail-grid">
            <FcDetailField label="Code">
              <code>{{ pool.code }}</code>
            </FcDetailField>
            <FcDetailField label="Name" :value="pool.name" />
            <FcDetailField
              v-if="pool.description"
              label="Description"
              :value="pool.description"
              span
            />
            <FcDetailField label="Rate Limit">
              <span v-if="pool.rateLimit != null">{{ pool.rateLimit }} / minute</span>
              <span v-else>Unlimited (concurrency-only)</span>
            </FcDetailField>
            <FcDetailField label="Concurrency" :value="pool.concurrency" />
            <FcDetailField label="Client Scope" :value="getScopeLabel(pool)" />
            <FcDetailField label="Status">
              <Tag :value="pool.status" :severity="getStatusSeverity(pool.status)" />
            </FcDetailField>
            <FcDetailField label="Created" :value="formatDate(pool.createdAt)" />
            <FcDetailField label="Updated" :value="formatDate(pool.updatedAt)" />
          </div>
        </template>
      </FcFormSection>

      <!-- Actions -->
      <FcFormSection v-if="!editing && pool.status !== 'ARCHIVED'" title="Actions" flat>
        <div class="action-items">
          <div v-if="pool.status !== 'ACTIVE'" class="action-item">
            <div class="action-info">
              <strong>Activate Pool</strong>
              <p>Enable this pool for processing dispatch jobs.</p>
            </div>
            <Button
              label="Activate"
              icon="pi pi-check-circle"
              severity="success"
              outlined
              @click="confirmActivate"
            />
          </div>

          <div v-if="pool.status === 'ACTIVE'" class="action-item">
            <div class="action-info">
              <strong>Suspend Pool</strong>
              <p>Temporarily stop processing jobs in this pool.</p>
            </div>
            <Button
              label="Suspend"
              icon="pi pi-pause"
              severity="warn"
              outlined
              @click="confirmSuspend"
            />
          </div>

          <div class="action-item">
            <div class="action-info">
              <strong>Delete Pool</strong>
              <p>Permanently deletes this pool. Cannot be undone.</p>
            </div>
            <Button
              label="Delete"
              icon="pi pi-trash"
              severity="danger"
              outlined
              @click="confirmDelete"
            />
          </div>
        </div>
      </FcFormSection>
    </template>

    <template v-if="editing" #footer>
      <FcFormActions :bordered="false">
        <Button v-if="dirty" label="Discard" severity="secondary" outlined @click="cancelEditing" />
        <Button label="Save" :disabled="!dirty" :loading="saving" @click="saveChanges" />
      </FcFormActions>
    </template>
  </EntityDrawer>
</template>

<style scoped>
.action-items {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.action-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  padding: 16px;
  background: #fafafa;
  border-radius: 8px;
  border: 1px solid #e5e7eb;
}

.action-info strong {
  display: block;
  margin-bottom: 4px;
}

.action-info p {
  margin: 0;
  font-size: 13px;
  color: #64748b;
}
</style>
