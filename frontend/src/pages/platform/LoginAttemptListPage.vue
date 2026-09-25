<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useCursorPagination } from "@/composables/useCursorPagination";
import { useListState } from "@/composables/useListState";
import { useTableFilters } from "@/composables/useTableFilters";
import {
	fetchLoginAttempts,
	type LoginAttempt,
} from "@/api/login-attempts";
import { usersApi } from "@/api/users";
import { oauthClientsApi } from "@/api/oauth-clients";

const listState = useListState(
	{
		filters: {
			attemptType: { type: "string", key: "attemptType" },
			outcome: { type: "string", key: "outcome" },
			identifier: { type: "string", key: "identifier" },
			dateFrom: { type: "string", key: "from" },
			dateTo: { type: "string", key: "to" },
		},
		pageSize: 100,
		debounceFields: ["identifier"],
	},
	() => {
		if (initialLoading.value) return;
		void cursor.reset();
	},
);
const { filters, pageSize, hasActiveFilters, clearFilters: clearListFilters } =
	listState;

// Lazy cursor table: the DataTable filter meta isn't bound — popup inputs
// write the listState refs directly and onChange resets the cursor. The
// debounced `identifier` field doubles as the toolbar's global search.
const { activeFilterCount } = useTableFilters(
	listState,
	[
		{ field: "attemptType", param: "attemptType" },
		{ field: "outcome", param: "outcome" },
		{ field: "dateFrom", param: "dateFrom" },
		{ field: "dateTo", param: "dateTo" },
	],
	{ globalParam: "identifier" },
);

const cursor = useCursorPagination<LoginAttempt>({
	fetchPage: async (after) => {
		const r = await fetchLoginAttempts({
			attemptType: filters.attemptType.value || undefined,
			outcome: filters.outcome.value || undefined,
			identifier: filters.identifier.value.trim() || undefined,
			dateFrom: filters.dateFrom.value || undefined,
			dateTo: filters.dateTo.value || undefined,
			after,
			pageSize: pageSize.value,
		});
		return {
			items: r.items,
			hasMore: r.hasMore,
			...(r.nextCursor !== undefined ? { nextCursor: r.nextCursor } : {}),
		};
	},
});
const attempts = cursor.items;
const loading = cursor.loading;
const initialLoading = ref(true);

async function clearFilters() {
	clearListFilters();
}

// Detail dialog
const selectedAttempt = ref<LoginAttempt | null>(null);
const showDetailDialog = ref(false);

const attemptTypeOptions = ["USER_LOGIN", "SERVICE_ACCOUNT_TOKEN"];
const outcomeOptions = ["SUCCESS", "FAILURE"];

/** A resolved navigation target for one of the dialog's id rows. */
interface ResolvedLink {
	name: string;
	params: Record<string, string>;
}

// Resolved once per dialog open (docs/spec/login-attempt-links.md F2). A
// failed lookup (403, 404, network) leaves the corresponding ref `null` —
// the row then renders as plain `<code>`, exactly as before, and is never
// surfaced as an error toast (both API calls suppress the global banner and
// the 401/403 session modal).
const principalLink = ref<ResolvedLink | null>(null);
const identifierLink = ref<ResolvedLink | null>(null);

const silent = { suppressGlobalErrorToast: true, suppressAuthErrorEvent: true };

// Bumped on every open, so a slow lookup for a previously opened attempt
// can never write its links onto the attempt now showing.
let resolveGeneration = 0;

async function resolveLinks(attempt: LoginAttempt) {
	const generation = ++resolveGeneration;
	const current = () => generation === resolveGeneration;
	principalLink.value = null;
	identifierLink.value = null;

	if (attempt.principalId) {
		try {
			const principal = await usersApi.get(attempt.principalId, silent);
			if (!current()) return;
			if (principal.type === "USER") {
				principalLink.value = { name: "user-detail", params: { id: attempt.principalId } };
			} else if (principal.type === "SERVICE" && principal.serviceAccountId) {
				principalLink.value = {
					name: "service-account-detail",
					params: { id: principal.serviceAccountId },
				};
			}
		} catch {
			// stays unresolved — the row renders as plain text.
		}
	}

	if (attempt.attemptType === "SERVICE_ACCOUNT_TOKEN") {
		try {
			const client = await oauthClientsApi.getByClientId(attempt.identifier, silent);
			if (!current()) return;
			identifierLink.value = { name: "oauth-client-detail", params: { id: client.id } };
		} catch {
			// stays unresolved — the row renders as plain text.
		}
	} else if (attempt.attemptType === "DEVELOPER_TOKEN") {
		// The identifier IS the USER principal id — same target as the
		// Principal ID row, whatever that resolved (or failed) to.
		identifierLink.value = principalLink.value;
	} else if (attempt.attemptType === "USER_LOGIN" && principalLink.value?.name === "user-detail") {
		identifierLink.value = principalLink.value;
	}
}

function viewDetails(attempt: LoginAttempt) {
	selectedAttempt.value = attempt;
	showDetailDialog.value = true;
	void resolveLinks(attempt);
}

function formatDateTime(isoString: string): string {
	return new Date(isoString).toLocaleString();
}

function formatAttemptType(type: string): string {
	return type === "USER_LOGIN" ? "User Login" : "Service Account";
}

function formatFailureReason(reason: string | null): string {
	if (!reason) return "";
	return reason
		.replace(/_/g, " ")
		.toLowerCase()
		.replace(/^./, (c) => c.toUpperCase());
}

function outcomeSeverity(outcome: string): string {
	return outcome === "SUCCESS" ? "success" : "danger";
}

function attemptTypeSeverity(type: string): string {
	return type === "USER_LOGIN" ? "info" : "secondary";
}

onMounted(async () => {
	await cursor.loadFirst();
	initialLoading.value = false;
});
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div>
        <h1 class="page-title">Login Attempts</h1>
        <p class="page-subtitle">Authentication attempt history for users and service accounts</p>
        <p class="page-note">
          SSO sign-ins appear when the platform accepts or refuses the identity provider's response. Failures at the
          identity provider itself (wrong password, MFA) are only in that provider's logs.
        </p>
      </div>
    </header>

    <!-- Data Table -->
    <div class="fc-card table-card">
      <div v-if="initialLoading" class="loading-container">
        <ProgressSpinner strokeWidth="3" />
      </div>

      <DataTable
        v-else
        :value="attempts"
        :loading="loading"
        size="small"
        @row-click="(e) => viewDetails(e.data)"
        :rowClass="() => 'clickable-row'"
      >
        <template #header>
          <FcTableToolbar
            v-model:search="filters.identifier.value"
            search-placeholder="Search by email or client_id..."
            :active-filter-count="activeFilterCount"
            :has-active-filters="hasActiveFilters"
            show-refresh
            @refresh="cursor.refresh"
            @clear-all="clearFilters"
          >
            <template #filters>
              <FcFormField label="Attempt Type">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.attemptType.value"
                    :options="attemptTypeOptions"
                    placeholder="All Types"
                    :showClear="true"
                    appendTo="self"
                  />
                </template>
              </FcFormField>
              <FcFormField label="Outcome">
                <template #default="{ id: fieldId }">
                  <Select
                    :id="fieldId"
                    v-model="filters.outcome.value"
                    :options="outcomeOptions"
                    placeholder="All Outcomes"
                    :showClear="true"
                    appendTo="self"
                  />
                </template>
              </FcFormField>
              <FcFormField label="From">
                <template #default="{ id: fieldId }">
                  <InputText
                    :id="fieldId"
                    v-model="filters.dateFrom.value"
                    type="datetime-local"
                  />
                </template>
              </FcFormField>
              <FcFormField label="To">
                <template #default="{ id: fieldId }">
                  <InputText
                    :id="fieldId"
                    v-model="filters.dateTo.value"
                    type="datetime-local"
                  />
                </template>
              </FcFormField>
            </template>
          </FcTableToolbar>
        </template>

        <Column field="attemptedAt" header="Time" style="width: 16%">
          <template #body="{ data }">
            <span class="time-text">{{ formatDateTime(data.attemptedAt) }}</span>
          </template>
        </Column>

        <Column field="attemptType" header="Type" style="width: 14%">
          <template #body="{ data }">
            <Tag
              :value="formatAttemptType(data.attemptType)"
              :severity="attemptTypeSeverity(data.attemptType)"
            />
          </template>
        </Column>

        <Column field="outcome" header="Outcome" style="width: 10%">
          <template #body="{ data }">
            <Tag :value="data.outcome" :severity="outcomeSeverity(data.outcome)" />
          </template>
        </Column>

        <Column field="identifier" header="Identifier" style="width: 22%">
          <template #body="{ data }">
            <code class="identifier-text">{{ data.identifier }}</code>
          </template>
        </Column>

        <Column field="failureReason" header="Failure Reason" style="width: 18%">
          <template #body="{ data }">
            <span v-if="data.failureReason" class="failure-reason">
              {{ formatFailureReason(data.failureReason) }}
            </span>
            <span v-else class="muted-text">—</span>
          </template>
        </Column>

        <Column field="ipAddress" header="IP Address" style="width: 14%">
          <template #body="{ data }">
            <code v-if="data.ipAddress" class="ip-text">{{ data.ipAddress }}</code>
            <span v-else class="muted-text">—</span>
          </template>
        </Column>

        <Column style="width: 6%">
          <template #body="{ data }">
            <Button
              icon="pi pi-eye"
              rounded
              text
              severity="secondary"
              v-tooltip.left="'View details'"
              @click.stop="viewDetails(data)"
            />
          </template>
        </Column>

        <template #empty>
          <div class="empty-message">
            <i class="pi pi-inbox"></i>
            <span>No login attempts found</span>
            <Button v-if="hasActiveFilters" label="Clear filters" link @click="clearFilters" />
          </div>
        </template>
      </DataTable>

      <!-- Cursor pager. iam_login_attempts is unbounded; we never count. -->
      <div class="cursor-pager">
        <Button
          icon="pi pi-angle-left"
          label="Newer"
          text
          :disabled="!cursor.hasPrev.value || cursor.loading.value"
          @click="cursor.loadPrev"
        />
        <span class="page-indicator">Page {{ cursor.page.value }}</span>
        <Button
          icon="pi pi-angle-right"
          iconPos="right"
          label="Older"
          text
          :disabled="!cursor.hasMore.value || cursor.loading.value"
          @click="cursor.loadNext"
        />
      </div>
    </div>

    <!-- Detail Dialog -->
    <Dialog
      v-model:visible="showDetailDialog"
      header="Login Attempt Details"
      :modal="true"
      :style="{ width: '600px' }"
      :closable="true"
    >
      <div v-if="selectedAttempt" class="detail-content">
        <div class="detail-grid">
          <div class="detail-row">
            <span class="detail-label">Time</span>
            <span class="detail-value">{{ formatDateTime(selectedAttempt.attemptedAt) }}</span>
          </div>

          <div class="detail-row">
            <span class="detail-label">Type</span>
            <Tag
              :value="formatAttemptType(selectedAttempt.attemptType)"
              :severity="attemptTypeSeverity(selectedAttempt.attemptType)"
            />
          </div>

          <div class="detail-row">
            <span class="detail-label">Outcome</span>
            <Tag
              :value="selectedAttempt.outcome"
              :severity="outcomeSeverity(selectedAttempt.outcome)"
            />
          </div>

          <div class="detail-row" v-if="selectedAttempt.failureReason">
            <span class="detail-label">Failure Reason</span>
            <span class="failure-reason detail-value">
              {{ formatFailureReason(selectedAttempt.failureReason) }}
            </span>
          </div>

          <div class="detail-row">
            <span class="detail-label">Identifier</span>
            <RouterLink
              v-if="identifierLink"
              :to="{ name: identifierLink.name, params: identifierLink.params }"
              @click="showDetailDialog = false"
            >
              <code class="identifier-text">{{ selectedAttempt.identifier }}</code>
            </RouterLink>
            <code v-else class="identifier-text">{{ selectedAttempt.identifier }}</code>
          </div>

          <div class="detail-row" v-if="selectedAttempt.principalId">
            <span class="detail-label">Principal ID</span>
            <RouterLink
              v-if="principalLink"
              :to="{ name: principalLink.name, params: principalLink.params }"
              @click="showDetailDialog = false"
            >
              <code class="identifier-text">{{ selectedAttempt.principalId }}</code>
            </RouterLink>
            <code v-else class="identifier-text">{{ selectedAttempt.principalId }}</code>
          </div>

          <div class="detail-row" v-if="selectedAttempt.ipAddress">
            <span class="detail-label">IP Address</span>
            <code class="ip-text">{{ selectedAttempt.ipAddress }}</code>
          </div>

          <div class="detail-row" v-if="selectedAttempt.userAgent">
            <span class="detail-label">User Agent</span>
            <span class="detail-value user-agent-text">{{ selectedAttempt.userAgent }}</span>
          </div>
        </div>
      </div>
    </Dialog>
  </div>
</template>

<style scoped>
.page-note {
  color: #94a3b8;
  margin-top: 4px;
  font-size: 12px;
  max-width: 60rem;
}

.cursor-pager {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 1rem;
  padding: 0.75rem 0 0.25rem;
}

.page-indicator {
  font-size: 0.875rem;
  color: var(--text-color-secondary);
  min-width: 4.5rem;
  text-align: center;
}

.table-card {
  padding: 0;
  overflow: hidden;
}

.loading-container {
  display: flex;
  justify-content: center;
  align-items: center;
  padding: 60px;
}

.time-text {
  font-size: 13px;
  color: #64748b;
}

.identifier-text {
  font-family: 'JetBrains Mono', monospace;
  font-size: 12px;
  background: #f1f5f9;
  padding: 2px 6px;
  border-radius: 4px;
  color: #475569;
}

.ip-text {
  font-family: 'JetBrains Mono', monospace;
  font-size: 12px;
  color: #64748b;
}

.failure-reason {
  font-size: 13px;
  color: #dc2626;
}

.muted-text {
  color: #94a3b8;
}

.empty-message {
  text-align: center;
  padding: 48px 24px;
  color: #64748b;
}

.empty-message i {
  font-size: 48px;
  display: block;
  margin-bottom: 16px;
  color: #cbd5e1;
}

.empty-message span {
  display: block;
  margin-bottom: 12px;
}

:deep(.clickable-row) {
  cursor: pointer;
  transition: background-color 0.15s;
}

:deep(.clickable-row:hover) {
  background-color: #f1f5f9 !important;
}

:deep(.p-datatable .p-datatable-thead > tr > th) {
  background: #f8fafc;
  color: #475569;
  font-weight: 600;
  font-size: 13px;
  text-transform: uppercase;
  letter-spacing: 0.025em;
}

/* Dialog styles */
.detail-content {
  padding: 8px 0;
}

.detail-grid {
  display: grid;
  gap: 16px;
}

.detail-row {
  display: flex;
  align-items: flex-start;
  gap: 16px;
}

.detail-label {
  min-width: 120px;
  font-size: 13px;
  font-weight: 500;
  color: #64748b;
  padding-top: 2px;
}

.detail-value {
  color: #1e293b;
}

.user-agent-text {
  font-size: 12px;
  color: #64748b;
  word-break: break-all;
}
</style>
