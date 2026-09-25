<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { toast } from "@/utils/errorBus";
import { useAuthStore } from "@/stores/auth";
import { useClientOptions } from "@/composables/useClientOptions";
import { usersApi } from "@/api/users";
import { passwordPolicyError } from "@/utils/passwordPolicy";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";

// Client-administrator create form. NOT the platform domain-check flow: a
// client admin creates CLIENT-scope users directly (usersApi.createClientUser
// → POST /principals with scope CLIENT), optionally setting an initial
// password. The target client is implicit unless the admin can act in more
// than one client.

const emit = defineEmits<{
	changed: [];
}>();

const authStore = useAuthStore();
const { ensureLoaded: ensureClients, getLabel: getClientLabel } =
	useClientOptions();

const saving = ref(false);
const created = ref(false);

// Form fields
const name = ref("");
const email = ref("");
const password = ref("");
const clientId = ref("");

// The clients this administrator can act in. With more than one, expose a
// target picker; with one, the client is implicit.
const clientOptions = computed(() =>
	authStore.accessibleClients.map((id) => ({
		label: getClientLabel(id),
		value: id,
	})),
);
const isMultiClient = computed(() => authStore.accessibleClients.length > 1);
const defaultClientId = computed(
	() => authStore.accessibleClients[0] ?? authStore.user?.clientId ?? "",
);

// Cheap dirty check: anything typed counts — until the create lands and we
// hand off to the detail drawer.
const dirty = computed(
	() => !created.value && (name.value !== "" || email.value !== ""),
);

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { goToList, replaceToDetail } = useDrawerRoute({
	listPath: "/client-administration/users",
	dirty,
});

onMounted(async () => {
	clientId.value = defaultClientId.value;
	await ensureClients();
});

async function createUser() {
	if (!email.value.trim() || !name.value.trim()) {
		toast.error("Error", "Name and email are required");
		return;
	}
	if (!clientId.value) {
		toast.error("Error", "Select a client");
		return;
	}
	if (password.value) {
		const policyErr = passwordPolicyError(password.value, {
			email: email.value,
			name: name.value,
		});
		if (policyErr) {
			toast.error("Weak password", policyErr);
			return;
		}
	}

	saving.value = true;
	try {
		const result = await usersApi.createClientUser({
			email: email.value.trim(),
			name: name.value.trim(),
			password: password.value || undefined,
			clientId: clientId.value,
		});
		created.value = true;
		toast.success("User created", `${name.value.trim()} was added`);
		emit("changed");
		replaceToDetail(result.id);
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
    subtitle="Create a user for your client"
    :dirty="dirty"
    @close="goToList()"
  >
    <FcFormSection title="User Information" flat>
      <div class="fc-form-grid">
        <FcFormField label="Name" required>
          <template #default="{ id: fieldId }">
            <InputText :id="fieldId" v-model="name" placeholder="e.g., John Smith" />
          </template>
        </FcFormField>

        <FcFormField label="Email" required>
          <template #default="{ id: fieldId }">
            <InputText
              :id="fieldId"
              v-model="email"
              type="email"
              placeholder="e.g., john.smith@example.com"
            />
          </template>
        </FcFormField>

        <FcFormField
          label="Password"
          span
          help="Leave blank to require the user to set it via a reset email."
        >
          <template #default="{ id: fieldId }">
            <Password
              :input-id="fieldId"
              v-model="password"
              toggleMask
              :feedback="false"
              :inputProps="{ autocomplete: 'new-password' }"
            />
          </template>
        </FcFormField>

        <FcFormField v-if="isMultiClient" label="Client" required span>
          <template #default="{ id: fieldId }">
            <Select
              :id="fieldId"
              v-model="clientId"
              :options="clientOptions"
              optionLabel="label"
              optionValue="value"
            />
          </template>
        </FcFormField>
      </div>
    </FcFormSection>

    <template #footer>
      <FcFormActions :bordered="false">
        <Button label="Cancel" severity="secondary" text :disabled="saving" @click="drawer?.close()" />
        <Button
          label="Create"
          icon="pi pi-check"
          :loading="saving"
          @click="createUser"
        />
      </FcFormActions>
    </template>
  </EntityDrawer>
</template>

<style scoped>
/* PrimeVue Password forwards classes to a wrapper whose inner <input> doesn't
 * carry this file's Vue scope attribute — deep-select the rendered elements so
 * the field fills the form-grid column (same trick as UserDetailBody.vue). */
:deep(.p-password) {
  width: 100%;
}
:deep(.p-password-input) {
  width: 100%;
}
</style>
