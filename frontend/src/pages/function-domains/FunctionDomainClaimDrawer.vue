<script setup lang="ts">
// Claim a zone for functions' public routes. An unscoped user picks the
// owner (platform or a client); a client-scoped user claims for its own
// client. The claim covers the hostname and every hostname under it.
import { computed, ref } from "vue";
import { useRoute } from "vue-router";
import { toast } from "@/utils/errorBus";
import { ApiError } from "@/api/client";
import { functionsApi } from "@/api/functions";
import { useAuthStore } from "@/stores/auth";
import { isUnscopedUser } from "@/stores/permissions";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";

const emit = defineEmits<{
	/** The owner the new claim belongs to, so the list can show it. */
	changed: [owner?: string];
}>();

const route = useRoute();
const authStore = useAuthStore();
const unscoped = computed(() => isUnscopedUser(authStore.user));

// Start from the owner the list is showing.
const listedOwner = typeof route.query["owner"] === "string" ? route.query["owner"] : "platform";
const hostname = ref("");
const platformOwned = ref(listedOwner === "platform");
const clientId = ref<string | null>(listedOwner === "platform" ? null : listedOwner);

const dirty = computed(() => hostname.value.trim() !== "");
const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { goToList, replaceToDetail } = useDrawerRoute({
	listPath: "/function-domains",
	dirty,
});

const claiming = ref(false);
const claimError = ref<string | null>(null);

const isValid = computed(
	() =>
		hostname.value.trim().length > 0 &&
		(!unscoped.value || platformOwned.value || !!clientId.value),
);

async function submit() {
	if (!isValid.value) return;
	claiming.value = true;
	claimError.value = null;
	try {
		const owner = unscoped.value
			? platformOwned.value
				? undefined
				: (clientId.value ?? undefined)
			: (authStore.user?.clientId ?? undefined);
		const domain = await functionsApi.claimDomain(
			{ hostname: hostname.value.trim(), clientId: owner },
			{ suppressGlobalErrorToast: true },
		);
		toast.success("Success", `${domain.hostname} claimed`);
		emit("changed", domain.owner);
		replaceToDetail(encodeURIComponent(domain.hostname));
	} catch (e) {
		claimError.value =
			e instanceof ApiError ? `${e.code ?? ""} ${e.message}`.trim() : "Failed to claim domain";
	} finally {
		claiming.value = false;
	}
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    title="Claim Domain"
    subtitle="A claim covers every hostname under it and is usable immediately"
    :dirty="dirty"
    @close="goToList()"
  >
    <FcFormSection title="Domain" flat>
      <FcFormField label="Hostname" required>
        <template #default="{ id: fieldId }">
          <InputText
            :id="fieldId"
            v-model="hostname"
            placeholder="functions.acme.com"
            autofocus
            @keyup.enter="submit"
          />
        </template>
        <template #help>
          Every hostname under it (e.g. <code>app.functions.acme.com</code>) is covered too.
        </template>
      </FcFormField>
    </FcFormSection>

    <FcFormSection v-if="unscoped" title="Owner" flat>
      <div class="checkbox-field">
        <Checkbox v-model="platformOwned" :binary="true" inputId="claimPlatformOwned" />
        <label for="claimPlatformOwned">Platform-owned</label>
      </div>
      <FcFormField v-if="!platformOwned" label="Client" required>
        <ClientSelect v-model="clientId" placeholder="Search for a client" />
      </FcFormField>
    </FcFormSection>

    <Message v-if="claimError" severity="error" :closable="false" class="error-message">
      {{ claimError }}
    </Message>

    <template #footer>
      <FcFormActions :bordered="false">
        <Button
          label="Cancel"
          icon="pi pi-times"
          severity="secondary"
          outlined
          :disabled="claiming"
          @click="drawer?.close()"
        />
        <Button
          label="Claim"
          icon="pi pi-check"
          :loading="claiming"
          :disabled="!isValid"
          @click="submit"
        />
      </FcFormActions>
    </template>
  </EntityDrawer>
</template>

<style scoped>
.checkbox-field {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
}

.checkbox-field label {
  margin: 0;
  cursor: pointer;
}

.error-message {
  margin-top: 16px;
}
</style>
