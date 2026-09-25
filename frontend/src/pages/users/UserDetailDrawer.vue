<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useRoute } from "vue-router";
import type { User } from "@/api/users";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import UserDetailBody from "@/pages/users/UserDetailBody.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";

const emit = defineEmits<{
	changed: [];
}>();

const route = useRoute();

const dirty = ref(false);

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { id, goToList } = useDrawerRoute({ listPath: "/users", dirty });

const loadedUser = ref<User | null>(null);

// ?edit=true deep link opens the info card in edit mode.
const autoEdit = computed(() => route.query["edit"] === "true");

// Reset chrome while switching between rows (the drawer instance is reused).
watch(id, () => {
	loadedUser.value = null;
});

const userType = computed(() => {
	const user = loadedUser.value;
	if (!user) return null;
	switch (user.scope) {
		case "ANCHOR":
			return { label: "Anchor", severity: "warn", icon: "pi pi-star" };
		case "PARTNER":
			return { label: "Partner", severity: "info", icon: undefined };
		case "CLIENT":
			return { label: "Client", severity: "secondary", icon: undefined };
	}
	// Fallback for older records without an explicit scope
	if (user.isAnchorUser) {
		return { label: "Anchor", severity: "warn", icon: "pi pi-star" };
	}
	return null;
});
</script>

<template>
  <EntityDrawer
    ref="drawer"
    :title="loadedUser?.name || 'User'"
    :subtitle="loadedUser?.email || undefined"
    size="wide"
    :dirty="dirty"
    @close="goToList()"
  >
    <template v-if="loadedUser" #header-extra>
      <Tag
        v-if="userType"
        :value="userType.label"
        :severity="userType.severity"
        :icon="userType.icon"
      />
      <Tag
        :value="loadedUser.active ? 'Active' : 'Inactive'"
        :severity="loadedUser.active ? 'success' : 'danger'"
      />
    </template>

    <UserDetailBody
      v-if="id"
      v-model:dirty="dirty"
      :user-id="id"
      :auto-edit="autoEdit"
      @loaded="loadedUser = $event"
      @changed="emit('changed')"
      @deleted="drawer?.close(true)"
    />
  </EntityDrawer>
</template>
