<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed, onMounted } from "vue";
import { usersApi, type EmailDomainCheckResponse } from "@/api/users";
import { clientsApi, type Client } from "@/api/clients";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";

const emit = defineEmits<{
	changed: [];
}>();

const saving = ref(false);

// Form fields
const email = ref("");
const name = ref("");
// Selected client when the domain check tells us we need one (partner-scope
// mappings, unmapped client-scope domains, or client-scope mappings without
// a primary client pinned).
const clientId = ref<string | null>(null);

// Cheap dirty check: anything typed counts.
const dirty = computed(() => email.value !== "" || name.value !== "");

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { goToList, replaceToDetail } = useDrawerRoute({
	listPath: "/users",
	dirty,
});

// Validation
const emailError = ref("");
const nameError = ref("");

// Email domain check
const domainCheck = ref<EmailDomainCheckResponse | null>(null);
const checkingDomain = ref(false);
const lastCheckedEmail = ref("");

// Clients (fetched once on mount). Used to populate the picker when the
// backend says we need a clientId. Picker filters to the response's
// `allowedClientIds` set when non-empty (partner-scope domains).
const clients = ref<Client[]>([]);
const loadingClients = ref(false);

// Whether the domain check has completed successfully (no errors, email validated)
const domainCheckComplete = computed(() => {
	return (
		domainCheck.value !== null && !checkingDomain.value && !emailError.value
	);
});

// True when the user is internally-authenticated. The admin never sets a
// password — the backend auto-sends a magic sign-in link on creation so the
// user picks their own. The flag drives the info copy on the form.
const isInternalAuth = computed(() => {
	if (!domainCheck.value) return false;
	return domainCheck.value.authProvider === "INTERNAL";
});

// Whether the form must collect a client id. The backend tells us in the
// domain-check response; this re-renders the picker whenever the response
// changes (e.g. user edits email).
const requiresClient = computed(() => {
	return domainCheck.value?.requiresClientId === true;
});

// Options for the client Select. When the backend constrains the choice
// (partner-scope domains), filter to that allow-list; otherwise the picker
// shows every active client.
const clientOptions = computed(() => {
	const allowed = domainCheck.value?.allowedClientIds ?? [];
	const filtered =
		allowed.length > 0
			? clients.value.filter((c) => allowed.includes(c.id))
			: clients.value;
	return filtered
		.filter((c) => c.status === "ACTIVE")
		.map((c) => ({ label: `${c.name} (${c.identifier})`, value: c.id }));
});

// Check if email already exists (blocking error)
const emailAlreadyExists = computed(() => {
	return domainCheck.value?.emailExists === true;
});

const isFormValid = computed(() => {
	// Block if email already exists
	if (emailAlreadyExists.value) return false;

	const baseValid =
		email.value &&
		name.value &&
		!emailError.value &&
		!nameError.value &&
		!checkingDomain.value &&
		domainCheckComplete.value; // Must have completed domain check

	// Client picker is required for partner / unmapped-client domains.
	if (requiresClient.value && !clientId.value) {
		return false;
	}

	return baseValid;
});

async function validateEmail() {
	if (!email.value) {
		emailError.value = "Email is required";
		domainCheck.value = null;
		return;
	}

	if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email.value)) {
		emailError.value = "Please enter a valid email address";
		domainCheck.value = null;
		return;
	}

	emailError.value = "";

	// Check email domain if email changed
	if (email.value !== lastCheckedEmail.value) {
		await checkEmailDomain();
	}
}

async function checkEmailDomain() {
	if (!email.value || !email.value.includes("@")) return;

	lastCheckedEmail.value = email.value;
	checkingDomain.value = true;
	// Clear previous check result while loading
	domainCheck.value = null;
	// Clear any stale client selection; the new domain's allow-list may
	// not include it, and an anchor domain doesn't take one at all.
	clientId.value = null;

	try {
		const result = await usersApi.checkEmailDomain(email.value);
		domainCheck.value = result;

		// If the domain constrains the client choice to a single allowed
		// id, pre-select it — saves a click for the common case.
		if (
			result.requiresClientId &&
			result.allowedClientIds.length === 1 &&
			result.allowedClientIds[0]
		) {
			clientId.value = result.allowedClientIds[0];
		}
	} catch (error) {
		console.error("Failed to check email domain:", error);
		domainCheck.value = null;
	} finally {
		checkingDomain.value = false;
	}
}

async function loadClients() {
	loadingClients.value = true;
	try {
		const res = await clientsApi.list({ status: "ACTIVE" });
		clients.value = res.clients;
	} catch (e) {
		console.error("Failed to load clients:", e);
	} finally {
		loadingClients.value = false;
	}
}

onMounted(loadClients);

function validateName() {
	if (!name.value) {
		nameError.value = "Name is required";
	} else {
		nameError.value = "";
	}
}

async function createUser() {
	// Validate all fields
	await validateEmail();
	validateName();

	if (!isFormValid.value) {
		return;
	}

	saving.value = true;
	try {
		// We never send a password. For INTERNAL users the backend
		// automatically emails a magic sign-in link so they pick their own;
		// for OIDC users the IdP owns credentials entirely. The admin never
		// sees or sets a password.
		const request: Parameters<typeof usersApi.create>[0] = {
			email: email.value,
			name: name.value,
		};

		// The backend defaults to CLIENT and never promotes scope from the
		// email domain on its own — echo the domain check's derived scope so
		// anchor/partner domains create what the form previewed.
		if (domainCheck.value?.derivedScope) {
			request.scope =
				domainCheck.value.derivedScope as (typeof request)["scope"];
		}

		// Include the picked clientId when the backend signalled it's
		// required (partner / unmapped-client domains).
		if (requiresClient.value && clientId.value) {
			request.clientId = clientId.value;
		}

		const user = await usersApi.create(request);

		toast.success(
			"User created",
			isInternalAuth.value
				? "We've emailed them a one-time sign-in link to set their password."
				: "They can sign in via their identity provider.",
		);

		emit("changed");
		replaceToDetail(user.id);
	} catch {
		// create errors surface via the global error toast
	} finally {
		saving.value = false;
	}
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    title="Add User"
    subtitle="Create a new platform user"
    :dirty="dirty"
    @close="goToList()"
  >
    <FcFormSection title="User Information" flat>
      <div class="fc-form-grid">
        <FcFormField label="Full Name" required :error="nameError || undefined">
          <template #default="{ id: fieldId }">
            <InputText
              :id="fieldId"
              v-model="name"
              placeholder="e.g., John Smith"
              :invalid="!!nameError"
              @blur="validateName"
            />
          </template>
        </FcFormField>

        <FcFormField
          label="Email Address"
          required
          :error="emailError || (emailAlreadyExists ? domainCheck?.warning : undefined)"
        >
          <template #default="{ id: fieldId }">
            <InputText
              :id="fieldId"
              v-model="email"
              type="email"
              placeholder="e.g., john.smith@example.com"
              :invalid="!!emailError || emailAlreadyExists"
              @blur="validateEmail"
            />
          </template>
          <template #help>
            <span v-if="checkingDomain" class="domain-checking">
              <i class="pi pi-spin pi-spinner"></i> Checking email...
            </span>
            <span v-else-if="domainCheck?.warning" class="domain-warning">
              <i class="pi pi-exclamation-triangle"></i> {{ domainCheck.warning }}
            </span>
            <span v-else-if="domainCheck?.info" class="domain-info">
              <i class="pi pi-info-circle"></i> {{ domainCheck.info }}
            </span>
          </template>
        </FcFormField>

        <FcFormField
          v-if="requiresClient"
          label="Client"
          required
          span
          :error="!loadingClients && clientOptions.length === 0
            ? 'No clients available to assign. Configure a client first.'
            : undefined"
        >
          <template #default="{ id: fieldId }">
            <Select
              :id="fieldId"
              v-model="clientId"
              :options="clientOptions"
              option-label="label"
              option-value="value"
              :placeholder="loadingClients ? 'Loading clients…' : 'Select a client'"
              :loading="loadingClients"
              :disabled="loadingClients || clientOptions.length === 0"
              show-clear
              filter
            />
          </template>
          <template #help>
            <span v-if="domainCheck?.derivedScope === 'PARTNER'" class="domain-info">
              <i class="pi pi-info-circle"></i>
              This partner domain restricts users to specific clients.
            </span>
            <span v-else class="domain-info">
              <i class="pi pi-info-circle"></i>
              Required for non-anchor users — sets the user's home client.
            </span>
          </template>
        </FcFormField>
      </div>
    </FcFormSection>

    <!-- Only show info message after domain check completes and email doesn't exist -->
    <Message
      v-if="domainCheckComplete && !emailAlreadyExists && isInternalAuth"
      severity="info"
      :closable="false"
      class="info-message"
    >
      <template #icon>
        <i class="pi pi-envelope"></i>
      </template>
      The user will be emailed a one-time sign-in link to set their own password
      — we never see or store an admin-set password.
      Scope: <strong>{{ domainCheck?.derivedScope }}</strong>.
    </Message>
    <Message
      v-else-if="domainCheckComplete && !emailAlreadyExists && !isInternalAuth"
      severity="info"
      :closable="false"
      class="info-message"
    >
      <template #icon>
        <i class="pi pi-info-circle"></i>
      </template>
      This user will authenticate via their organization's identity provider
      ({{ domainCheck?.authProvider }}) — no password to set.
      Scope: <strong>{{ domainCheck?.derivedScope }}</strong>.
    </Message>

    <template #footer>
      <FcFormActions :bordered="false">
        <Button label="Cancel" severity="secondary" text :disabled="saving" @click="drawer?.close()" />
        <Button
          label="Create User"
          icon="pi pi-check"
          :loading="saving"
          :disabled="!isFormValid"
          @click="createUser"
        />
      </FcFormActions>
    </template>
  </EntityDrawer>
</template>

<style scoped>
.info-message {
  margin-top: 16px;
}

.domain-checking {
  color: #64748b;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.domain-info {
  color: #0d9488;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.domain-warning {
  color: #d97706;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
</style>
