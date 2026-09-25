<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { FilterMatchMode } from "@primevue/core/api";
import {
	emailDomainMappingsApi,
	type EmailDomainMapping,
} from "@/api/email-domain-mappings";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";

const router = useRouter();
const route = useRoute();
const mappings = ref<EmailDomainMapping[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);

const listState = useListState({
	filters: {
		q: { type: "string", key: "q" },
		scopeType: { type: "string", key: "scope" },
		idp: { type: "string", key: "idp" },
	},
	debounceFields: ["q", "idp"],
});
const { filters } = listState;

const { filters: tableFilters, activeFilterCount, clearAll } = useTableFilters(
	listState,
	[
		{ field: "scopeType", param: "scopeType" },
		{
			field: "identityProviderName",
			param: "idp",
			matchMode: FilterMatchMode.CONTAINS,
		},
	],
);

const scopeTypeFilterOptions = [
	{ label: "Anchor", value: "ANCHOR" },
	{ label: "Partner", value: "PARTNER" },
	{ label: "Client", value: "CLIENT" },
];

// Delete dialog state
const showDeleteDialog = ref(false);
const mappingToDelete = ref<EmailDomainMapping | null>(null);
const deleteLoading = ref(false);

onMounted(async () => {
	await loadMappings();
});

async function loadMappings() {
	loading.value = true;
	error.value = null;
	try {
		const response = await emailDomainMappingsApi.list();
		mappings.value = response.mappings;
	} catch (e) {
		error.value =
			e instanceof Error ? e.message : "Failed to load email domain mappings";
	} finally {
		loading.value = false;
	}
}

function openDetail(id: string, edit = false) {
	void router.push({
		path: `/authentication/email-domain-mappings/${id}`,
		query: edit ? { ...route.query, edit: "true" } : route.query,
	});
}

function openCreate() {
	void router.push({
		path: "/authentication/email-domain-mappings/new",
		query: route.query,
	});
}

function confirmDelete(mapping: EmailDomainMapping) {
	mappingToDelete.value = mapping;
	showDeleteDialog.value = true;
}

async function deleteMapping() {
	if (!mappingToDelete.value) return;

	deleteLoading.value = true;

	try {
		await emailDomainMappingsApi.delete(mappingToDelete.value.id);
		mappings.value = mappings.value.filter(
			(m) => m.id !== mappingToDelete.value?.id,
		);
		showDeleteDialog.value = false;
		toast.success(
			"Success",
			`Email domain mapping for "${mappingToDelete.value.emailDomain}" deleted`,
		);
	} catch {
		// delete errors surface via the global error toast
	} finally {
		deleteLoading.value = false;
		mappingToDelete.value = null;
	}
}

function getScopeTypeSeverity(scopeType: string) {
	switch (scopeType) {
		case "ANCHOR":
			return "danger";
		case "PARTNER":
			return "warn";
		case "CLIENT":
			return "info";
		default:
			return "secondary";
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
        <h1 class="page-title">Email Domain Mappings</h1>
        <p class="page-subtitle">Map email domains to identity providers and define user scope.</p>
      </div>
      <Button label="Add Domain Mapping" icon="pi pi-plus" @click="openCreate" />
    </header>

    <Message v-if="error" severity="error" class="error-message">{{ error }}</Message>

    <div class="fc-card">
      <DataTable
        :value="mappings"
        :loading="loading"
        :filters="tableFilters"
        :globalFilterFields="['emailDomain', 'scopeType', 'identityProviderName']"
        paginator
        :rows="100"
        :rowsPerPageOptions="[50, 100, 250, 500]"
        stripedRows
        rowHover
        :rowClass="() => 'clickable-row'"
        @row-click="(e) => openDetail(e.data.id)"
      >
        <template #header>
          <FcTableToolbar
            v-model:search="filters.q.value"
            search-placeholder="Search domains..."
            :active-filter-count="activeFilterCount"
            :has-active-filters="listState.hasActiveFilters.value"
            @clear-all="clearAll"
          >
            <template #filters>
              <FcFormField label="Scope Type">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.scopeType.value"
                    :options="scopeTypeFilterOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All scopes"
                    showClear
                    appendTo="self"
                  />
                </template>
              </FcFormField>
              <FcFormField label="Identity Provider">
                <template #default="{ id: fieldId }">
                  <InputText
                    :id="fieldId"
                    v-model="filters.idp.value"
                    placeholder="Filter by provider name"
                  />
                </template>
              </FcFormField>
            </template>
          </FcTableToolbar>
        </template>
        <template #empty>No email domain mappings found</template>

        <Column field="emailDomain" header="Email Domain" sortable>
          <template #body="{ data }">
            <span class="domain-name">{{ data.emailDomain }}</span>
          </template>
        </Column>
        <Column field="identityProviderName" header="Identity Provider" sortable>
          <template #body="{ data }">
            <!-- The wire only enriches identityProviderName; there is no
                 identityProviderType field (the old Tag here never rendered). -->
            <span class="provider-name">{{ data.identityProviderName || 'Unknown' }}</span>
          </template>
        </Column>
        <Column field="scopeType" header="Scope Type" sortable>
          <template #body="{ data }">
            <Tag :value="data.scopeType" :severity="getScopeTypeSeverity(data.scopeType)" />
          </template>
        </Column>
        <Column header="Primary Client">
          <template #body="{ data }">
            <!-- The wire carries primaryClientId only (no primaryClientName —
                 the old binding here never rendered); the detail page resolves
                 the display name from the clients list. -->
            <span v-if="data.primaryClientId" class="client-name">{{
              data.primaryClientId
            }}</span>
            <span v-else class="text-muted">-</span>
          </template>
        </Column>
        <Column field="createdAt" header="Created" sortable>
          <template #body="{ data }">
            {{ formatDate(data.createdAt) }}
          </template>
        </Column>
        <Column header="Actions" style="width: 100px">
          <template #body="{ data }">
            <div class="action-buttons">
              <Button
                v-tooltip="'Edit'"
                icon="pi pi-pencil"
                text
                rounded
                @click.stop="openDetail(data.id, true)"
              />
              <Button
                v-tooltip="'Delete'"
                icon="pi pi-trash"
                text
                rounded
                severity="danger"
                @click.stop="confirmDelete(data)"
              />
            </div>
          </template>
        </Column>
      </DataTable>
    </div>

    <!-- Delete Confirmation Dialog -->
    <Dialog
      v-model:visible="showDeleteDialog"
      header="Delete Email Domain Mapping"
      modal
      :style="{ width: '450px' }"
    >
      <div class="dialog-content">
        <p>
          Are you sure you want to delete the mapping for
          <strong>{{ mappingToDelete?.emailDomain }}</strong
          >?
        </p>

        <Message severity="warn" :closable="false" class="warning-message">
          Users from this domain will no longer be able to authenticate.
        </Message>
      </div>

      <template #footer>
        <Button label="Cancel" text :disabled="deleteLoading" @click="showDeleteDialog = false" />
        <Button
          label="Delete"
          icon="pi pi-trash"
          severity="danger"
          :loading="deleteLoading"
          @click="deleteMapping"
        />
      </template>
    </Dialog>

    <!-- Drawer outlet: detail/create child routes render over this list -->
    <RouterView v-slot="{ Component }">
      <component :is="Component" @changed="loadMappings" />
    </RouterView>
  </div>
</template>

<style scoped>
.error-message {
  margin-bottom: 16px;
}

.domain-name {
  font-weight: 500;
  color: #1e293b;
}

.provider-name {
  color: #1e293b;
}

.client-name {
  color: #1e293b;
}

.text-muted {
  color: #94a3b8;
}

.dialog-content {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.warning-message {
  margin: 0;
}

.action-buttons {
  display: flex;
  flex-wrap: nowrap;
  gap: 0;
}
</style>
