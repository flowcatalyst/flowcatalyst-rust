<script setup lang="ts">
import { ref } from "vue";
import { useRouter } from "vue-router";
import type Popover from "primevue/popover";
import { useAuthStore } from "@/stores/auth";
import { logout, checkEmailDomain } from "@/api/auth";

defineProps<{
	collapsed: boolean;
}>();

const authStore = useAuthStore();
const router = useRouter();

const popover = ref<InstanceType<typeof Popover> | null>(null);
const showIdpNotice = ref(false);

function toggle(event: MouseEvent) {
	popover.value?.toggle(event);
}

async function handleResetPassword() {
	popover.value?.hide();

	const email = authStore.user?.email;
	if (!email) return;

	try {
		const domainCheck = await checkEmailDomain(email);
		if (domainCheck.authMethod === "external") {
			showIdpNotice.value = true;
		} else {
			await router.push("/auth/reset-password");
		}
	} catch {
		await router.push("/auth/reset-password");
	}
}

async function handleLogout() {
	popover.value?.hide();
	await logout();
}

function closePopover() {
	popover.value?.hide();
}
</script>

<template>
  <div class="sidebar-profile">
    <button
      v-if="!collapsed"
      class="profile-trigger"
      @click="toggle"
    >
      <div class="profile-avatar">
        {{ authStore.userInitials }}
      </div>
      <div class="profile-info">
        <span class="profile-name">{{ authStore.displayName }}</span>
        <span class="profile-email">{{ authStore.user?.email }}</span>
      </div>
      <i class="pi pi-chevron-up"></i>
    </button>
    <button
      v-else
      v-tooltip.right="authStore.displayName"
      class="profile-trigger profile-trigger-collapsed"
      @click="toggle"
    >
      <div class="profile-avatar">
        {{ authStore.userInitials }}
      </div>
    </button>

    <Popover ref="popover" class="sidebar-profile-popover">
      <div class="menu-header">
        <div class="profile-avatar large">
          {{ authStore.userInitials }}
        </div>
        <div class="user-details">
          <span class="name">{{ authStore.displayName }}</span>
          <span class="email">{{ authStore.user?.email }}</span>
          <span v-if="authStore.isPlatformAdmin" class="badge admin">Platform Admin</span>
        </div>
      </div>

      <div class="menu-divider"></div>

      <div class="menu-items">
        <RouterLink to="/profile" class="menu-item" @click="closePopover">
          <i class="pi pi-user"></i>
          <span>Profile</span>
        </RouterLink>
        <button class="menu-item" @click="handleResetPassword">
          <i class="pi pi-key"></i>
          <span>Reset Password</span>
        </button>
      </div>

      <div class="menu-divider"></div>

      <div class="menu-items">
        <button class="menu-item danger" @click="handleLogout">
          <i class="pi pi-sign-out"></i>
          <span>Sign Out</span>
        </button>
      </div>

      <div class="menu-divider"></div>

      <div class="menu-footer">
        <span>Version</span>
        <span class="version-number">0.0.1</span>
      </div>
    </Popover>

    <Dialog
      v-model:visible="showIdpNotice"
      modal
      header="External Identity Provider"
      :style="{ width: '400px' }"
    >
      <p>Your account is managed by an external identity provider.</p>
      <p>To reset your password, please visit your organization's identity provider portal.</p>
      <template #footer>
        <Button label="Close" severity="secondary" @click="showIdpNotice = false" />
      </template>
    </Dialog>
  </div>
</template>

<style scoped>
.profile-trigger {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 8px;
  background: none;
  border: none;
  border-radius: 8px;
  cursor: pointer;
  transition: background 0.2s ease;
  text-align: left;
}

.profile-trigger:hover {
  background: rgba(255, 255, 255, 0.08);
}

.profile-trigger-collapsed {
  justify-content: center;
  padding: 8px 0;
}

.profile-avatar {
  width: 32px;
  height: 32px;
  border-radius: 50%;
  background: linear-gradient(135deg, #0967d2 0%, #47a3f3 100%);
  color: white;
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: 600;
  font-size: 13px;
  flex-shrink: 0;
}

.profile-avatar.large {
  width: 48px;
  height: 48px;
  font-size: 18px;
}

.profile-info {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
}

.profile-name {
  font-weight: 500;
  color: rgba(255, 255, 255, 0.85);
  font-size: 13px;
  line-height: 1.3;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.profile-email {
  font-size: 11px;
  color: rgba(255, 255, 255, 0.5);
  line-height: 1.3;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.profile-trigger > i {
  color: rgba(255, 255, 255, 0.5);
  font-size: 11px;
  flex-shrink: 0;
}

/* Popover content (teleported panel body carries this component's scope) */
.menu-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 16px;
}

.user-details {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.user-details .name {
  font-weight: 600;
  color: #1e293b;
  font-size: 15px;
}

.user-details .email {
  font-size: 13px;
  color: #64748b;
}

.badge {
  display: inline-flex;
  align-items: center;
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 11px;
  font-weight: 500;
  margin-top: 4px;
  width: fit-content;
}

.badge.admin {
  background: #dbeafe;
  color: #1d4ed8;
}

.menu-divider {
  height: 1px;
  background: #e2e8f0;
}

.menu-items {
  padding: 8px;
}

.menu-item {
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
  padding: 10px 12px;
  background: none;
  border: none;
  border-radius: 6px;
  cursor: pointer;
  color: #475569;
  font-size: 14px;
  text-decoration: none;
  transition: all 0.15s ease;
  text-align: left;
}

.menu-item:hover {
  background: #f1f5f9;
  color: #1e293b;
}

.menu-item i {
  font-size: 16px;
  width: 20px;
  text-align: center;
}

.menu-item.danger {
  color: #dc2626;
}

.menu-item.danger:hover {
  background: #fef2f2;
  color: #b91c1c;
}

.menu-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 10px 16px;
  font-size: 12px;
  color: #94a3b8;
}

.menu-footer .version-number {
  color: #64748b;
}
</style>

<style>
/* Panel chrome — the Popover's own elements don't carry this component's scope */
.sidebar-profile-popover.p-popover {
  min-width: 280px;
}

.sidebar-profile-popover .p-popover-content {
  padding: 0;
}
</style>
