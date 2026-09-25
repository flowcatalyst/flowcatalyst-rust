<script setup lang="ts">
import { toast } from "@/utils/errorBus";
import { ref, computed } from "vue";
import { clientsApi } from "@/api/clients";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";

const emit = defineEmits<{
	changed: [];
}>();

const name = ref("");
const identifier = ref("");
const submitting = ref(false);
const errorMessage = ref<string | null>(null);

// Cheap dirty check: anything typed counts.
const dirty = computed(() => name.value !== "" || identifier.value !== "");

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { goToList, replaceToDetail } = useDrawerRoute({
	listPath: "/clients",
	dirty,
});

const IDENTIFIER_PATTERN = /^[a-z][a-z0-9-]*$/;

const isIdentifierValid = computed(() => {
	return !identifier.value || IDENTIFIER_PATTERN.test(identifier.value);
});

const isFormValid = computed(() => {
	return (
		name.value.trim().length > 0 &&
		name.value.length <= 255 &&
		identifier.value.length >= 2 &&
		identifier.value.length <= 100 &&
		IDENTIFIER_PATTERN.test(identifier.value)
	);
});

async function onSubmit() {
	if (!isFormValid.value) return;

	submitting.value = true;
	errorMessage.value = null;

	try {
		const client = await clientsApi.create({
			name: name.value,
			identifier: identifier.value,
		});
		toast.success("Success", "Client created");
		emit("changed");
		replaceToDetail(client.id);
	} catch (e) {
		errorMessage.value =
			e instanceof Error ? e.message : "Failed to create client";
	} finally {
		submitting.value = false;
	}
}
</script>

<template>
  <EntityDrawer
    ref="drawer"
    title="Create Client"
    subtitle="Add a new customer client to the platform"
    :dirty="dirty"
    @close="goToList()"
  >
    <div class="form-field">
      <label>Name <span class="required">*</span></label>
      <InputText
        v-model="name"
        placeholder="Client display name"
        class="full-width"
        :invalid="name.length > 255"
      />
      <small class="char-count">{{ name.length }} / 255</small>
    </div>

    <div class="form-field">
      <label>Identifier <span class="required">*</span></label>
      <InputText
        v-model="identifier"
        placeholder="client-slug"
        class="full-width"
        :invalid="!!(identifier && !isIdentifierValid)"
      />
      <small v-if="identifier && !isIdentifierValid" class="p-error">
        Lowercase letters, numbers, hyphens only. Must start with a letter.
      </small>
      <small v-else class="hint">
        Unique identifier used in URLs and configurations (2-100 characters)
      </small>
    </div>

    <Message v-if="errorMessage" severity="error" class="error-message">
      {{ errorMessage }}
    </Message>

    <template #footer>
      <FcFormActions :bordered="false">
        <Button
          label="Cancel"
          icon="pi pi-times"
          severity="secondary"
          outlined
          :disabled="submitting"
          @click="drawer?.close()"
        />
        <Button
          label="Create Client"
          icon="pi pi-check"
          :loading="submitting"
          :disabled="!isFormValid"
          @click="onSubmit"
        />
      </FcFormActions>
    </template>
  </EntityDrawer>
</template>

<style scoped>
.form-field {
  margin-bottom: 20px;
}

.form-field > label {
  display: block;
  font-weight: 500;
  margin-bottom: 6px;
}

.form-field .required {
  color: #ef4444;
}

.full-width {
  width: 100%;
}

.char-count {
  display: block;
  text-align: right;
  font-size: 12px;
  color: #94a3b8;
  margin-top: 4px;
}

.hint {
  display: block;
  font-size: 12px;
  color: #64748b;
  margin-top: 4px;
}

.error-message {
  margin-bottom: 16px;
}
</style>
