<script setup lang="ts">
import { ref, computed } from "vue";
import { useRouter } from "vue-router";
import { useToast } from "primevue/usetoast";
import { usersApi, type EmailDomainCheckResponse } from "@/api/users";
import { getErrorMessage } from "@/utils/errors";

const router = useRouter();
const toast = useToast();

const saving = ref(false);

// Form fields
const email = ref("");
const password = ref("");
const confirmPassword = ref("");
const name = ref("");

// Validation
const emailError = ref("");
const passwordError = ref("");
const nameError = ref("");

// Email domain check
const domainCheck = ref<EmailDomainCheckResponse | null>(null);
const checkingDomain = ref(false);
const lastCheckedEmail = ref("");

// Whether the domain check has completed successfully (no errors, email validated)
const domainCheckComplete = computed(() => {
	return (
		domainCheck.value !== null && !checkingDomain.value && !emailError.value
	);
});

// Determine if user needs internal authentication (password)
const requiresPassword = computed(() => {
	// Don't show password fields until domain check confirms internal auth
	if (!domainCheck.value) return false;
	return domainCheck.value.authProvider === "INTERNAL";
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

	if (requiresPassword.value) {
		return (
			baseValid &&
			password.value &&
			password.value === confirmPassword.value &&
			!passwordError.value
		);
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

	try {
		const result = await usersApi.checkEmailDomain(email.value);
		domainCheck.value = result;

		// Clear password fields if switching to OIDC (no password needed)
		if (result.authProvider !== "INTERNAL") {
			password.value = "";
			confirmPassword.value = "";
			passwordError.value = "";
		}
	} catch (error) {
		console.error("Failed to check email domain:", error);
		domainCheck.value = null;
	} finally {
		checkingDomain.value = false;
	}
}

function validatePassword() {
	// Skip validation if password not required (OIDC users)
	if (!requiresPassword.value) {
		passwordError.value = "";
		return;
	}

	if (!password.value) {
		passwordError.value = "Password is required";
	} else if (password.value.length < 8) {
		passwordError.value = "Password must be at least 8 characters";
	} else if (
		confirmPassword.value &&
		password.value !== confirmPassword.value
	) {
		passwordError.value = "Passwords do not match";
	} else {
		passwordError.value = "";
	}
}

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
	validatePassword();
	validateName();

	if (!isFormValid.value) {
		return;
	}

	saving.value = true;
	try {
		// Build request - only include password for internal auth users
		const request: Parameters<typeof usersApi.create>[0] = {
			email: email.value,
			name: name.value,
		};

		if (requiresPassword.value) {
			request.password = password.value;
		}

		// Create the user (client will be auto-detected from email domain on backend)
		const user = await usersApi.create(request);

		toast.add({
			severity: "success",
			summary: "Success",
			detail: "User created successfully",
			life: 3000,
		});

		// Redirect to user detail/edit page
		router.push(`/users/${user.id}`);
	} catch (e: unknown) {
		toast.add({
			severity: "error",
			summary: "Error",
			detail: getErrorMessage(e, "Failed to create user"),
			life: 5000,
		});
	} finally {
		saving.value = false;
	}
}

function cancel() {
	router.push("/users");
}
</script>

<template>
  <div class="page-container">
    <header class="page-header">
      <div class="header-left">
        <Button
          icon="pi pi-arrow-left"
          text
          rounded
          severity="secondary"
          @click="cancel"
          v-tooltip.right="'Back to users'"
        />
        <div>
          <h1 class="page-title">Add User</h1>
          <p class="page-subtitle">Create a new platform user</p>
        </div>
      </div>
      <div class="header-right">
        <Button label="Cancel" severity="secondary" text @click="cancel" />
        <Button
          label="Create User"
          icon="pi pi-check"
          :loading="saving"
          :disabled="!isFormValid"
          @click="createUser"
        />
      </div>
    </header>

    <div class="fc-card">
      <h2 class="card-title">User Information</h2>

      <div class="form-grid">
        <div class="form-field">
          <label for="name">Full Name <span class="required">*</span></label>
          <InputText
            id="name"
            v-model="name"
            placeholder="e.g., John Smith"
            class="w-full"
            :invalid="!!nameError"
            @blur="validateName"
          />
          <small v-if="nameError" class="p-error">{{ nameError }}</small>
        </div>

        <div class="form-field">
          <label for="email">Email Address <span class="required">*</span></label>
          <InputText
            id="email"
            v-model="email"
            type="email"
            placeholder="e.g., john.smith@example.com"
            class="w-full"
            :invalid="!!emailError || emailAlreadyExists"
            @blur="validateEmail"
          />
          <small v-if="emailError" class="p-error">{{ emailError }}</small>
          <small v-else-if="checkingDomain" class="domain-checking">
            <i class="pi pi-spin pi-spinner"></i> Checking email...
          </small>
          <small v-else-if="emailAlreadyExists" class="p-error">
            <i class="pi pi-times-circle"></i> {{ domainCheck?.warning }}
          </small>
          <small v-else-if="domainCheck?.warning" class="domain-warning">
            <i class="pi pi-exclamation-triangle"></i> {{ domainCheck.warning }}
          </small>
          <small v-else-if="domainCheck?.info" class="domain-info">
            <i class="pi pi-info-circle"></i> {{ domainCheck.info }}
          </small>
        </div>

        <template v-if="requiresPassword">
          <div class="form-field">
            <label for="password">Password <span class="required">*</span></label>
            <Password
              id="password"
              v-model="password"
              placeholder="Minimum 12 characters"
              class="w-full"
              :invalid="!!passwordError"
              :feedback="true"
              toggleMask
              appendTo="self"
              @blur="validatePassword"
            />
            <small v-if="passwordError" class="p-error">{{ passwordError }}</small>
          </div>

          <div class="form-field">
            <label for="confirmPassword">Confirm Password <span class="required">*</span></label>
            <Password
              id="confirmPassword"
              v-model="confirmPassword"
              placeholder="Re-enter password"
              class="w-full"
              :invalid="password !== confirmPassword && confirmPassword !== ''"
              :feedback="false"
              toggleMask
              @blur="validatePassword"
            />
            <small v-if="confirmPassword && password !== confirmPassword" class="p-error">
              Passwords do not match
            </small>
          </div>
        </template>
      </div>
    </div>

    <!-- Only show info message after domain check completes and email doesn't exist -->
    <Message
      v-if="domainCheckComplete && !emailAlreadyExists && requiresPassword"
      severity="info"
      :closable="false"
      class="info-message"
    >
      <template #icon>
        <i class="pi pi-info-circle"></i>
      </template>
      The user will be created with internal authentication. Client access will be determined based
      on the email domain. After creation, you can manage additional client access on the user
      detail page.
    </Message>
    <Message
      v-else-if="domainCheckComplete && !emailAlreadyExists && !requiresPassword"
      severity="info"
      :closable="false"
      class="info-message"
    >
      <template #icon>
        <i class="pi pi-info-circle"></i>
      </template>
      This user will authenticate via their organization's identity provider ({{
        domainCheck?.authProvider
      }}). No password is required. Client access will be determined based on the email domain.
    </Message>
  </div>
</template>

<style scoped>
.header-left {
  display: flex;
  align-items: center;
  gap: 12px;
}

.header-right {
  display: flex;
  align-items: center;
  gap: 12px;
}

.fc-card {
  margin-bottom: 24px;
}

.card-title {
  font-size: 16px;
  font-weight: 600;
  color: #1e293b;
  margin: 0 0 20px 0;
}

.form-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 20px;
}

.form-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.form-field label {
  font-size: 13px;
  font-weight: 500;
  color: #475569;
}

.required {
  color: #ef4444;
}

.w-full {
  width: 100%;
}

:deep(.p-password) {
  width: 100%;
}

:deep(.p-password-input) {
  width: 100%;
}

/* Fix password strength panel positioning */
:deep(.p-password-panel) {
  margin-top: 8px;
}

:deep(.p-password-meter) {
  margin-top: 8px;
}

.info-message {
  margin-top: 0;
}

.domain-checking {
  color: #64748b;
  display: flex;
  align-items: center;
  gap: 6px;
}

.domain-info {
  color: #0d9488;
  display: flex;
  align-items: center;
  gap: 6px;
}

.domain-warning {
  color: #d97706;
  display: flex;
  align-items: center;
  gap: 6px;
}

@media (max-width: 768px) {
  .form-grid {
    grid-template-columns: 1fr;
  }
}
</style>
