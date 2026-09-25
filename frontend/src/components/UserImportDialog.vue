<script setup lang="ts">
import { ref, computed } from "vue";
import { toast } from "@/utils/errorBus";
import { getErrorMessage } from "@/utils/errors";
import {
	usersApi,
	type BulkImportUserRow,
	type BulkImportResponse,
} from "@/api/users";

// Reusable CSV user-import dialog (trigger button + modal). Used by both the
// client-administration page (client admins, scoped to their own clients) and
// the platform users page (platform admins, who pick any client). The platform
// API applies the drop rules — existing emails and emails whose domain belongs
// to another client are skipped and reported per row.
const props = defineProps<{
	/** Clients the admin may import into. One → auto-selected; many → a picker. */
	clientOptions: { label: string; value: string }[];
	/** Pre-selected client (omit to force the admin to choose). */
	defaultClientId?: string;
	/** Trigger button label. */
	label?: string;
}>();

const emit = defineEmits<{ (e: "imported"): void }>();

const visible = ref(false);
const fileName = ref("");
const rows = ref<BulkImportUserRow[]>([]);
const clientId = ref("");
const importing = ref(false);
const error = ref("");
const result = ref<BulkImportResponse | null>(null);

// Show the picker whenever the choice isn't already forced to a single client.
const needsClientSelect = computed(() => props.clientOptions.length !== 1);

function open() {
	fileName.value = "";
	rows.value = [];
	clientId.value =
		props.defaultClientId ??
		(props.clientOptions.length === 1 ? (props.clientOptions[0]?.value ?? "") : "");
	error.value = "";
	result.value = null;
	visible.value = true;
}

function downloadTemplate() {
	const csv =
		"Full Name,Email,Roles (| separated)\n" +
		"Jane Doe,jane.doe@example.com,role-one|role-two\n" +
		"John Smith,john.smith@example.com,\n";
	const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
	const url = URL.createObjectURL(blob);
	const a = document.createElement("a");
	a.href = url;
	a.download = "user-import-template.csv";
	a.click();
	URL.revokeObjectURL(url);
}

// Minimal CSV field parser: handles quoted fields and escaped quotes ("").
function parseCsvLine(line: string): string[] {
	const out: string[] = [];
	let cur = "";
	let inQuotes = false;
	for (let i = 0; i < line.length; i++) {
		const c = line[i];
		if (inQuotes) {
			if (c === '"') {
				if (line[i + 1] === '"') {
					cur += '"';
					i++;
				} else {
					inQuotes = false;
				}
			} else {
				cur += c;
			}
		} else if (c === '"') {
			inQuotes = true;
		} else if (c === ",") {
			out.push(cur);
			cur = "";
		} else {
			cur += c;
		}
	}
	out.push(cur);
	return out;
}

function parseCsv(text: string): BulkImportUserRow[] {
	const lines = text.split(/\r?\n/).filter((l) => l.trim() !== "");
	if (lines.length === 0) return [];
	const first = (lines[0] ?? "").toLowerCase();
	const startIdx = first.includes("email") && first.includes("name") ? 1 : 0;
	const parsed: BulkImportUserRow[] = [];
	for (let i = startIdx; i < lines.length; i++) {
		const cols = parseCsvLine(lines[i] ?? "");
		const name = (cols[0] ?? "").trim();
		const email = (cols[1] ?? "").trim();
		const roles = (cols[2] ?? "")
			.split("|")
			.map((r) => r.trim())
			.filter(Boolean);
		if (!name && !email) continue;
		parsed.push({ name, email, roles });
	}
	return parsed;
}

async function onFileChange(event: Event) {
	error.value = "";
	result.value = null;
	rows.value = [];
	const input = event.target as HTMLInputElement;
	const file = input.files?.[0];
	if (!file) return;
	fileName.value = file.name;
	try {
		rows.value = parseCsv(await file.text());
		if (rows.value.length === 0) {
			error.value = "No user rows found in the file.";
		}
	} catch (e) {
		error.value = getErrorMessage(e, "Could not read the file.");
	}
}

async function submitImport() {
	error.value = "";
	if (!clientId.value) {
		error.value = "Select a client.";
		return;
	}
	if (rows.value.length === 0) {
		error.value = "Choose a CSV file with at least one user.";
		return;
	}
	importing.value = true;
	try {
		result.value = await usersApi.bulkImport(clientId.value, rows.value);
		const r = result.value;
		toast.success(
			"Import complete",
			`${r.created} created, ${r.skipped} skipped, ${r.failed} failed`,
		);
		emit("imported");
	} catch (e) {
		error.value = getErrorMessage(e, "Import failed.");
	} finally {
		importing.value = false;
	}
}

// Rows that were not created, for the per-row report. Existing + dropped are
// "skips" (amber); genuine errors are red.
const reportRows = computed(
	() => result.value?.results.filter((r) => r.status !== "created") ?? [],
);
function rowSeverity(status: string): "warn" | "danger" {
	return status === "exists" || status === "dropped" ? "warn" : "danger";
}
</script>

<template>
	<Button
		:label="label ?? 'Import CSV'"
		icon="pi pi-upload"
		outlined
		@click="open"
	/>

	<Dialog
		v-model:visible="visible"
		header="Import users from CSV"
		modal
		:style="{ width: '42rem' }"
	>
		<div class="dialog-form">
			<p class="dialog-note">
				Upload a CSV with columns <strong>Full Name</strong>,
				<strong>Email</strong>, and <strong>Roles</strong>
				(pipe&#8209;separated, e.g. <code>role-one|role-two</code>). New users
				are created and emailed an invite to set their password.
				<strong>Existing users</strong>, and users whose email
				<strong>domain belongs to another client</strong>, are skipped and
				listed below.
			</p>

			<div class="import-toolbar">
				<Button
					label="Download template"
					icon="pi pi-download"
					text
					size="small"
					@click="downloadTemplate"
				/>
			</div>

			<div v-if="needsClientSelect" class="field">
				<label for="imp-client">Client</label>
				<Select
					id="imp-client"
					v-model="clientId"
					:options="clientOptions"
					optionLabel="label"
					optionValue="value"
					filter
					placeholder="Select a client"
					class="w-full"
				/>
			</div>

			<div class="field">
				<label for="imp-file">CSV file</label>
				<input
					id="imp-file"
					type="file"
					accept=".csv,text/csv"
					class="file-input"
					@change="onFileChange"
				/>
				<small v-if="fileName" class="hint">
					{{ fileName }} — {{ rows.length }} user(s) parsed.
				</small>
			</div>

			<p v-if="error" class="error-text">{{ error }}</p>

			<div v-if="result" class="import-results">
				<div class="import-summary">
					<Tag :value="`${result.created} created`" severity="success" />
					<Tag :value="`${result.skipped} skipped`" severity="warn" />
					<Tag
						:value="`${result.failed} failed`"
						:severity="result.failed ? 'danger' : 'secondary'"
					/>
				</div>
				<DataTable
					v-if="reportRows.length"
					:value="reportRows"
					size="small"
					class="import-table"
				>
					<Column field="row" header="Row" style="width: 4rem" />
					<Column field="email" header="Email" />
					<Column header="Result">
						<template #body="{ data }">
							<Tag :value="data.status" :severity="rowSeverity(data.status)" />
						</template>
					</Column>
					<Column field="message" header="Detail" />
				</DataTable>
			</div>
		</div>
		<template #footer>
			<Button
				label="Close"
				text
				severity="secondary"
				@click="visible = false"
			/>
			<Button
				label="Import"
				icon="pi pi-upload"
				:loading="importing"
				:disabled="rows.length === 0"
				@click="submitImport"
			/>
		</template>
	</Dialog>
</template>

<style scoped>
.dialog-form {
	display: flex;
	flex-direction: column;
	gap: 16px;
}
.dialog-note {
	font-size: 13px;
	color: #475569;
	line-height: 1.5;
	margin: 0;
}
.import-toolbar {
	margin-top: -4px;
}
.field {
	display: flex;
	flex-direction: column;
	gap: 6px;
}
.field label {
	font-size: 13px;
	font-weight: 500;
	color: #475569;
}
.file-input {
	font-size: 14px;
}
.hint {
	color: #64748b;
}
.error-text {
	color: #dc2626;
	font-size: 13px;
	margin: 0;
}
.import-results {
	display: flex;
	flex-direction: column;
	gap: 10px;
}
.import-summary {
	display: flex;
	gap: 8px;
}
.w-full {
	width: 100%;
}
</style>
