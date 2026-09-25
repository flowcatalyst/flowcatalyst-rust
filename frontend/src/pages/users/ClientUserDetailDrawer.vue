<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useRoute } from "vue-router";
import type { User } from "@/api/users";
import EntityDrawer from "@/components/drawer/EntityDrawer.vue";
import UserDetailBody from "@/pages/users/UserDetailBody.vue";
import { useDrawerRoute } from "@/composables/useDrawerRoute";

// Client-administrator user detail: the shared UserDetailBody in clientScoped
// mode (no scope/client re-association, no client-access section, no
// all-applications toggle, no delete). Every row here is CLIENT scope, so the
// header shows only the Active tag — no type tag.

const emit = defineEmits<{
	changed: [];
}>();

const route = useRoute();

const dirty = ref(false);

const drawer = ref<InstanceType<typeof EntityDrawer> | null>(null);
const { id, goToList } = useDrawerRoute({
	listPath: "/client-administration/users",
	dirty,
});

const loadedUser = ref<User | null>(null);

// ?edit=true deep link opens the info card in edit mode.
const autoEdit = computed(() => route.query["edit"] === "true");

// Reset chrome while switching between rows (the drawer instance is reused).
watch(id, () => {
	loadedUser.value = null;
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
        :value="loadedUser.active ? 'Active' : 'Inactive'"
        :severity="loadedUser.active ? 'success' : 'danger'"
      />
    </template>

    <!-- delete can't fire in clientScoped mode; the handler stays for symmetry -->
    <UserDetailBody
      v-if="id"
      v-model:dirty="dirty"
      :user-id="id"
      client-scoped
      :auto-edit="autoEdit"
      @loaded="loadedUser = $event"
      @changed="emit('changed')"
      @deleted="drawer?.close(true)"
    />
  </EntityDrawer>
</template>
