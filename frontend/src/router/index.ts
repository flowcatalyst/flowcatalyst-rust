import { createRouter, createWebHistory } from "vue-router";
import { authGuard, guestGuard, createRoutePermissionGuard } from "./guards";

const router = createRouter({
	history: createWebHistory(),
	routes: [
		// Standalone logout confirmation page. Other applications redirect
		// users here after their own sign-out so the user can also end their
		// Identity Server session and log back in as a different identity.
		// Top-level path (not under /auth) to avoid colliding with the
		// backend's POST /auth/logout endpoint, which would otherwise return
		// 405 on the browser's GET request and never reach the SPA fallback.
		// No guard — must be reachable whether the user is authenticated or not.
		{
			path: "/logout",
			name: "logout",
			component: () => import("@/pages/auth/LogoutPage.vue"),
		},
		// Portal-plane login (no layout, NO guest guard: portal identities
		// are a separate population — a signed-in platform user must still
		// be able to open a portal sign-in link).
		{
			path: "/portal/login",
			name: "portal-login",
			component: () => import("@/pages/auth/PortalLoginPage.vue"),
		},
		// Auth routes (no layout, guest only)
		{
			path: "/auth",
			children: [
				{
					path: "login",
					name: "login",
					component: () => import("@/pages/auth/LoginPage.vue"),
					beforeEnter: guestGuard,
				},
				{
					path: "forgot-password",
					name: "forgot-password",
					component: () =>
						import("@/pages/auth/ForgotPasswordPage.vue"),
					beforeEnter: guestGuard,
				},
				{
					path: "reset-password",
					name: "reset-password",
					component: () =>
						import("@/pages/auth/ResetPasswordPage.vue"),
					beforeEnter: guestGuard,
				},
				// Invite framing of the same page ("set your password" —
				// first-time invites, incl. portal identities). No guest
				// guard: invitees are often not platform users at all.
				{
					path: "set-password",
					name: "set-password",
					component: () =>
						import("@/pages/auth/ResetPasswordPage.vue"),
				},
				{
					path: "",
					redirect: "/auth/login",
				},
			],
		},
		// Protected routes (with layout)
		{
			path: "/",
			component: () => import("@/layouts/MainLayout.vue"),
			beforeEnter: authGuard,
			children: [
				{
					path: "",
					redirect: "/dashboard",
				},
				{
					path: "dashboard",
					name: "dashboard",
					component: () => import("@/pages/DashboardPage.vue"),
					meta: { scope: "anchor" },
				},
				// Applications
				{
					path: "applications",
					name: "applications",
					component: () =>
						import("@/pages/applications/ApplicationListPage.vue"),
					children: [
						{
							path: "new",
							name: "application-create",
							component: () =>
								import("@/pages/applications/ApplicationCreateDrawer.vue"),
						},
						{
							path: ":id",
							name: "application-detail",
							component: () =>
								import("@/pages/applications/ApplicationDetailDrawer.vue"),
						},
					],
				},
				// Clients
				{
					path: "clients",
					name: "clients",
					component: () => import("@/pages/clients/ClientListPage.vue"),
					children: [
						{
							path: "new",
							name: "client-create",
							component: () =>
								import("@/pages/clients/ClientCreateDrawer.vue"),
						},
						{
							path: ":id",
							name: "client-detail",
							component: () =>
								import("@/pages/clients/ClientDetailDrawer.vue"),
						},
					],
				},
				// Login branding is a full page, not a drawer — navigate away
				// from the client detail drawer.
				{
					path: "clients/:id/theme",
					name: "client-theme",
					component: () => import("@/pages/clients/ClientLoginThemePage.vue"),
				},
				// Users (platform / anchor scope — full user administration).
				// Detail/create render in a right-side drawer over the list; children
				// inherit the parent's meta.scope via vue-router's merged meta.
				{
					path: "users",
					name: "users",
					component: () => import("@/pages/users/UserListPage.vue"),
					meta: { scope: "anchor" },
					children: [
						{
							path: "new",
							name: "user-create",
							component: () => import("@/pages/users/UserCreateDrawer.vue"),
						},
						{
							path: ":id",
							name: "user-detail",
							component: () => import("@/pages/users/UserDetailDrawer.vue"),
						},
					],
				},
				// Client-scoped user management (client-administrators) — manage only
				// their own client's users, with role assignment bounded to the
				// client's applications. Children inherit meta.scope.
				{
					path: "client-administration/users",
					name: "client-users",
					component: () => import("@/pages/users/ClientUsersPage.vue"),
					meta: { scope: "client" },
					children: [
						{
							path: "new",
							name: "client-user-create",
							component: () =>
								import("@/pages/users/ClientUserCreateDrawer.vue"),
						},
						{
							path: ":id",
							name: "client-user-detail",
							component: () =>
								import("@/pages/users/ClientUserDetailDrawer.vue"),
						},
					],
				},
				// Service Accounts
				{
					path: "identity/service-accounts",
					name: "service-accounts",
					component: () =>
						import("@/pages/service-accounts/ServiceAccountListPage.vue"),
					children: [
						{
							path: "new",
							name: "service-account-create",
							component: () =>
								import("@/pages/service-accounts/ServiceAccountCreateDrawer.vue"),
						},
						{
							path: ":id",
							name: "service-account-detail",
							component: () =>
								import("@/pages/service-accounts/ServiceAccountDetailDrawer.vue"),
						},
					],
				},
				// Portal Users — per-client management of the portal identity
				// plane. Anchors pick a client; client admins holding the
				// portal-administrator role see their own client(s).
				{
					path: "identity/portal-users",
					name: "portal-users",
					component: () =>
						import("@/pages/portal/PortalUsersPage.vue"),
				},
				// Portal Apps — the named portals a client runs; OAuth clients
				// link to one and portal users are granted per app.
				{
					path: "identity/portal-apps",
					name: "portal-apps",
					component: () =>
						import("@/pages/portal/PortalAppsPage.vue"),
				},
				// Developer Users — designate existing users as developers and
				// manage their self-service API credentials. Granting the role is
				// anchor-only (it's a platform role); matches User Management/Roles.
				{
					path: "identity/developer-users",
					name: "developer-users",
					component: () =>
						import("@/pages/developer/DeveloperUsersListPage.vue"),
					meta: { scope: "anchor" },
				},
				// Authorization - Roles
				{
					path: "authorization/roles",
					name: "roles",
					component: () => import("@/pages/authorization/RoleListPage.vue"),
					meta: { scope: "anchor" },
					children: [
						{
							path: ":roleName",
							name: "role-detail",
							component: () =>
								import("@/pages/authorization/RoleDetailDrawer.vue"),
						},
					],
				},
				// Role editor stays a full page (carve-out); the 3-segment path
				// wins over the nested :roleName child above.
				{
					path: "authorization/roles/:roleName/edit",
					name: "role-edit",
					component: () => import("@/pages/authorization/RoleEditPage.vue"),
					meta: { scope: "anchor" },
				},
				// Authorization - Permissions
				{
					path: "authorization/permissions",
					name: "permissions",
					component: () =>
						import("@/pages/authorization/PermissionListPage.vue"),
					meta: { scope: "anchor" },
				},
				// Authentication - Identity Providers
				{
					path: "authentication/identity-providers",
					name: "identity-providers",
					component: () =>
						import(
							"@/pages/authentication/identity-providers/IdentityProviderListPage.vue"
						),
					children: [
						{
							path: "new",
							name: "identity-provider-create",
							component: () =>
								import(
									"@/pages/authentication/identity-providers/IdentityProviderCreateDrawer.vue"
								),
						},
						{
							path: ":id",
							name: "identity-provider-detail",
							component: () =>
								import(
									"@/pages/authentication/identity-providers/IdentityProviderDetailDrawer.vue"
								),
						},
					],
				},
				// Authentication - Email Domain Mappings
				// Detail/create render in a right-side drawer over the list
				// (nested children keep the list mounted underneath).
				{
					path: "authentication/email-domain-mappings",
					name: "email-domain-mappings",
					component: () =>
						import(
							"@/pages/authentication/email-domains/EmailDomainMappingListPage.vue"
						),
					children: [
						{
							path: "new",
							name: "email-domain-mapping-create",
							component: () =>
								import(
									"@/pages/authentication/email-domains/EmailDomainMappingCreateDrawer.vue"
								),
						},
						{
							path: ":id",
							name: "email-domain-mapping-detail",
							component: () =>
								import(
									"@/pages/authentication/email-domains/EmailDomainMappingDetailDrawer.vue"
								),
						},
					],
				},
				// Authentication - lost-device reset approvals (client-admin queue).
				// The :id form is the deep link from the approval email.
				{
					path: "authentication/reset-approvals/:id?",
					name: "reset-approvals",
					component: () =>
						import("@/pages/authentication/ResetApprovalsPage.vue"),
				},
				// Authentication - OAuth Clients
				{
					path: "authentication/oauth-clients",
					name: "oauth-clients",
					component: () =>
						import("@/pages/authentication/OAuthClientListPage.vue"),
					children: [
						{
							path: "new",
							name: "oauth-client-create",
							component: () =>
								import("@/pages/authentication/OAuthClientCreateDrawer.vue"),
						},
						{
							path: ":id",
							name: "oauth-client-detail",
							component: () =>
								import("@/pages/authentication/OAuthClientDetailDrawer.vue"),
						},
					],
				},
				// Legacy redirects
				{
					path: "roles",
					redirect: "/authorization/roles",
				},
				{
					path: "authentication/domain-idps",
					redirect: "/authentication/identity-providers",
				},
				{
					path: "authentication/anchor-domains",
					redirect: "/authentication/email-domain-mappings",
				},
				// Event Types
				{
					path: "event-types",
					name: "event-types",
					component: () => import("@/pages/event-types/EventTypeListPage.vue"),
					children: [
						{
							path: "create",
							name: "event-type-create",
							component: () =>
								import("@/pages/event-types/EventTypeCreateDrawer.vue"),
						},
						{
							path: ":id",
							name: "event-type-detail",
							component: () =>
								import("@/pages/event-types/EventTypeDetailDrawer.vue"),
						},
					],
				},
				// Add-schema stays a full page (carve-out); the 3-segment path wins
				// over the nested :id child above.
				{
					path: "event-types/:id/add-schema",
					name: "event-type-add-schema",
					component: () =>
						import("@/pages/event-types/EventTypeAddSchemaPage.vue"),
				},
				// Scheduled Jobs
				{
					path: "scheduled-jobs",
					name: "scheduled-jobs",
					component: () =>
						import("@/pages/scheduled-jobs/ScheduledJobListPage.vue"),
					children: [
						{
							path: "create",
							name: "scheduled-job-create",
							component: () =>
								import("@/pages/scheduled-jobs/ScheduledJobCreateDrawer.vue"),
						},
						{
							path: ":id",
							name: "scheduled-job-detail",
							component: () =>
								import("@/pages/scheduled-jobs/ScheduledJobDetailDrawer.vue"),
						},
					],
				},
				{
					path: "scheduled-jobs/:id/instances",
					name: "scheduled-job-instances",
					component: () =>
						import(
							"@/pages/scheduled-jobs/ScheduledJobInstanceListPage.vue"
						),
				},
				{
					path: "scheduled-jobs/instances/:instanceId",
					name: "scheduled-job-instance-detail",
					component: () =>
						import(
							"@/pages/scheduled-jobs/ScheduledJobInstanceDetailPage.vue"
						),
				},
				// Subscriptions — detail/create render in a right-side drawer over
				// the list (nested children keep the list mounted underneath).
				{
					path: "subscriptions",
					name: "subscriptions",
					component: () =>
						import("@/pages/subscriptions/SubscriptionListPage.vue"),
					children: [
						{
							path: "new",
							name: "subscription-create",
							component: () =>
								import("@/pages/subscriptions/SubscriptionCreateDrawer.vue"),
						},
						{
							path: ":id",
							name: "subscription-detail",
							component: () =>
								import("@/pages/subscriptions/SubscriptionDetailDrawer.vue"),
						},
					],
				},
				// Connections
				{
					path: "connections",
					name: "connections",
					component: () =>
						import("@/pages/connections/ConnectionListPage.vue"),
					children: [
						{
							path: "new",
							name: "connection-create",
							component: () =>
								import("@/pages/connections/ConnectionCreateDrawer.vue"),
						},
						{
							path: ":id",
							name: "connection-detail",
							component: () =>
								import("@/pages/connections/ConnectionDetailDrawer.vue"),
						},
					],
				},
				// Dispatch Pools
				{
					path: "dispatch-pools",
					name: "dispatch-pools",
					component: () =>
						import("@/pages/dispatch-pools/DispatchPoolListPage.vue"),
					children: [
						{
							path: "new",
							name: "dispatch-pool-create",
							component: () =>
								import("@/pages/dispatch-pools/DispatchPoolCreateDrawer.vue"),
						},
						{
							path: ":id",
							name: "dispatch-pool-detail",
							component: () =>
								import("@/pages/dispatch-pools/DispatchPoolDetailDrawer.vue"),
						},
					],
				},
				// Dispatch Jobs
				{
					path: "dispatch-jobs",
					name: "dispatch-jobs",
					component: () =>
						import("@/pages/dispatch-jobs/DispatchJobListPage.vue"),
					children: [
						{
							path: ":id",
							name: "dispatch-job-detail",
							component: () =>
								import("@/pages/dispatch-jobs/DispatchJobDetailDrawer.vue"),
						},
					],
				},
				// Events
				{
					path: "events",
					name: "events",
					component: () => import("@/pages/events/EventListPage.vue"),
					children: [
						{
							path: ":id",
							name: "event-detail",
							component: () =>
								import("@/pages/events/EventDetailDrawer.vue"),
						},
					],
				},
				// Platform - CORS Origins
				{
					path: "platform/cors",
					name: "cors-origins",
					component: () => import("@/pages/platform/CorsOriginsPage.vue"),
				},
				// Platform - Audit Log
				{
					path: "platform/audit-log",
					name: "audit-log",
					component: () => import("@/pages/platform/AuditLogListPage.vue"),
				},
				// Platform - Documentation: published platform pages + app-synced
				// pages ({source} is "platform" or an application code).
				{
					path: "platform/docs/:source?/:slug?",
					name: "platform-docs",
					component: () => import("@/pages/platform/DocsPage.vue"),
				},
				// Platform - Login Attempts
				{
					path: "platform/login-attempts",
					name: "login-attempts",
					component: () =>
						import("@/pages/platform/LoginAttemptListPage.vue"),
				},
				// Platform - Settings
				{
					path: "platform/settings/theme",
					name: "theme-settings",
					component: () =>
						import("@/pages/platform/settings/LoginThemeSettingsPage.vue"),
				},
				{
					path: "platform/settings/names",
					name: "names-settings",
					component: () =>
						import("@/pages/platform/settings/PlatformNamesSettingsPage.vue"),
				},
				// Platform - Debug
				{
					path: "platform/debug/events",
					name: "debug-raw-events",
					component: () =>
						import("@/pages/platform/debug/RawEventListPage.vue"),
				},
				{
					path: "platform/debug/dispatch-jobs",
					name: "debug-raw-dispatch-jobs",
					component: () =>
						import("@/pages/platform/debug/RawDispatchJobListPage.vue"),
				},
				// Processes (workflow / Mermaid documentation)
				{
					path: "processes",
					name: "processes",
					component: () => import("@/pages/processes/ProcessListPage.vue"),
					children: [
						{
							path: ":id",
							name: "process-detail",
							component: () =>
								import("@/pages/processes/ProcessDetailDrawer.vue"),
						},
					],
				},
				// Process editor stays a full page (carve-out); static "create" and
				// the 3-segment edit path win over the nested :id child above.
				{
					path: "processes/create",
					name: "process-create",
					component: () =>
						import("@/pages/processes/ProcessCreatePage.vue"),
				},
				{
					path: "processes/:id/edit",
					name: "process-edit",
					component: () =>
						import("@/pages/processes/ProcessEditPage.vue"),
				},
				// Developer portal
				{
					path: "developer",
					name: "developer",
					component: () =>
						import("@/pages/developer/DeveloperApplicationsListPage.vue"),
				},
				{
					path: "developer/applications/:id",
					name: "developer-application-detail",
					component: () =>
						import("@/pages/developer/DeveloperApplicationDetailPage.vue"),
				},
				{
					path: "developer/applications/:id/versions",
					name: "developer-application-versions",
					component: () =>
						import("@/pages/developer/DeveloperApiVersionsPage.vue"),
				},
				// Profile
				{
					path: "profile",
					name: "profile",
					component: () => import("@/pages/ProfilePage.vue"),
				},
			],
		},
		// Catch-all redirect
		{
			path: "/:pathMatch(.*)*",
			redirect: "/dashboard",
		},
	],
});

// Register global permission guard
router.beforeEach(createRoutePermissionGuard());

export default router;
