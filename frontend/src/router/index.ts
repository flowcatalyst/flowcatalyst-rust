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
				},
				// Applications
				{
					path: "applications",
					name: "applications",
					component: () =>
						import("@/pages/applications/ApplicationListPage.vue"),
				},
				{
					path: "applications/new",
					name: "application-create",
					component: () =>
						import("@/pages/applications/ApplicationCreatePage.vue"),
				},
				{
					path: "applications/:id",
					name: "application-detail",
					component: () =>
						import("@/pages/applications/ApplicationDetailPage.vue"),
				},
				// Clients
				{
					path: "clients",
					name: "clients",
					component: () => import("@/pages/clients/ClientListPage.vue"),
				},
				{
					path: "clients/new",
					name: "client-create",
					component: () => import("@/pages/clients/ClientCreatePage.vue"),
				},
				{
					path: "clients/:id",
					name: "client-detail",
					component: () => import("@/pages/clients/ClientDetailPage.vue"),
				},
				// Users
				{
					path: "users",
					name: "users",
					component: () => import("@/pages/users/UserListPage.vue"),
				},
				{
					path: "users/new",
					name: "user-create",
					component: () => import("@/pages/users/UserCreatePage.vue"),
				},
				{
					path: "users/:id",
					name: "user-detail",
					component: () => import("@/pages/users/UserDetailPage.vue"),
				},
				// Service Accounts
				{
					path: "identity/service-accounts",
					name: "service-accounts",
					component: () =>
						import("@/pages/service-accounts/ServiceAccountListPage.vue"),
				},
				{
					path: "identity/service-accounts/new",
					name: "service-account-create",
					component: () =>
						import("@/pages/service-accounts/ServiceAccountCreatePage.vue"),
				},
				{
					path: "identity/service-accounts/:id",
					name: "service-account-detail",
					component: () =>
						import("@/pages/service-accounts/ServiceAccountDetailPage.vue"),
				},
				// Authorization - Roles
				{
					path: "authorization/roles",
					name: "roles",
					component: () => import("@/pages/authorization/RoleListPage.vue"),
				},
				{
					path: "authorization/roles/:roleName",
					name: "role-detail",
					component: () => import("@/pages/authorization/RoleDetailPage.vue"),
				},
				{
					path: "authorization/roles/:roleName/edit",
					name: "role-edit",
					component: () => import("@/pages/authorization/RoleEditPage.vue"),
				},
				// Authorization - Permissions
				{
					path: "authorization/permissions",
					name: "permissions",
					component: () =>
						import("@/pages/authorization/PermissionListPage.vue"),
				},
				// Authentication - Identity Providers
				{
					path: "authentication/identity-providers",
					name: "identity-providers",
					component: () =>
						import(
							"@/pages/authentication/identity-providers/IdentityProviderListPage.vue"
						),
				},
				{
					path: "authentication/identity-providers/new",
					name: "identity-provider-create",
					component: () =>
						import(
							"@/pages/authentication/identity-providers/IdentityProviderCreatePage.vue"
						),
				},
				{
					path: "authentication/identity-providers/:id",
					name: "identity-provider-detail",
					component: () =>
						import(
							"@/pages/authentication/identity-providers/IdentityProviderDetailPage.vue"
						),
				},
				// Authentication - Email Domain Mappings
				{
					path: "authentication/email-domain-mappings",
					name: "email-domain-mappings",
					component: () =>
						import(
							"@/pages/authentication/email-domains/EmailDomainMappingListPage.vue"
						),
				},
				{
					path: "authentication/email-domain-mappings/new",
					name: "email-domain-mapping-create",
					component: () =>
						import(
							"@/pages/authentication/email-domains/EmailDomainMappingCreatePage.vue"
						),
				},
				{
					path: "authentication/email-domain-mappings/:id",
					name: "email-domain-mapping-detail",
					component: () =>
						import(
							"@/pages/authentication/email-domains/EmailDomainMappingDetailPage.vue"
						),
				},
				// Authentication - OAuth Clients
				{
					path: "authentication/oauth-clients",
					name: "oauth-clients",
					component: () =>
						import("@/pages/authentication/OAuthClientListPage.vue"),
				},
				{
					path: "authentication/oauth-clients/new",
					name: "oauth-client-create",
					component: () =>
						import("@/pages/authentication/OAuthClientCreatePage.vue"),
				},
				{
					path: "authentication/oauth-clients/:id",
					name: "oauth-client-detail",
					component: () =>
						import("@/pages/authentication/OAuthClientDetailPage.vue"),
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
				},
				{
					path: "event-types/create",
					name: "event-type-create",
					component: () =>
						import("@/pages/event-types/EventTypeCreatePage.vue"),
				},
				{
					path: "event-types/:id",
					name: "event-type-detail",
					component: () =>
						import("@/pages/event-types/EventTypeDetailPage.vue"),
				},
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
				},
				{
					path: "scheduled-jobs/create",
					name: "scheduled-job-create",
					component: () =>
						import("@/pages/scheduled-jobs/ScheduledJobCreatePage.vue"),
				},
				{
					path: "scheduled-jobs/:id",
					name: "scheduled-job-detail",
					component: () =>
						import("@/pages/scheduled-jobs/ScheduledJobDetailPage.vue"),
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
				// Subscriptions
				{
					path: "subscriptions",
					name: "subscriptions",
					component: () =>
						import("@/pages/subscriptions/SubscriptionListPage.vue"),
				},
				{
					path: "subscriptions/new",
					name: "subscription-create",
					component: () =>
						import("@/pages/subscriptions/SubscriptionCreatePage.vue"),
				},
				{
					path: "subscriptions/:id",
					name: "subscription-detail",
					component: () =>
						import("@/pages/subscriptions/SubscriptionDetailPage.vue"),
				},
				// Connections
				{
					path: "connections",
					name: "connections",
					component: () =>
						import("@/pages/connections/ConnectionListPage.vue"),
				},
				{
					path: "connections/new",
					name: "connection-create",
					component: () =>
						import("@/pages/connections/ConnectionCreatePage.vue"),
				},
				{
					path: "connections/:id",
					name: "connection-detail",
					component: () =>
						import("@/pages/connections/ConnectionDetailPage.vue"),
				},
				// Dispatch Pools
				{
					path: "dispatch-pools",
					name: "dispatch-pools",
					component: () =>
						import("@/pages/dispatch-pools/DispatchPoolListPage.vue"),
				},
				{
					path: "dispatch-pools/new",
					name: "dispatch-pool-create",
					component: () =>
						import("@/pages/dispatch-pools/DispatchPoolCreatePage.vue"),
				},
				{
					path: "dispatch-pools/:id",
					name: "dispatch-pool-detail",
					component: () =>
						import("@/pages/dispatch-pools/DispatchPoolDetailPage.vue"),
				},
				// Dispatch Jobs
				{
					path: "dispatch-jobs",
					name: "dispatch-jobs",
					component: () =>
						import("@/pages/dispatch-jobs/DispatchJobListPage.vue"),
				},
				// Functions
				{
					path: "functions",
					name: "functions",
					component: () => import("@/pages/functions/FunctionListPage.vue"),
				},
				{
					path: "functions/new",
					name: "function-create",
					component: () =>
						import("@/pages/functions/FunctionCreatePage.vue"),
				},
				{
					path: "functions/:address",
					name: "function-detail",
					component: () =>
						import("@/pages/functions/FunctionDetailPage.vue"),
				},
				{
					path: "functions/:address/manifest",
					name: "function-manifest-editor",
					component: () =>
						import("@/pages/functions/FunctionManifestEditorPage.vue"),
				},
				{
					path: "function-domains",
					name: "function-domains",
					component: () =>
						import("@/pages/function-domains/FunctionDomainListPage.vue"),
				},
				{
					path: "function-domains/:hostname",
					name: "function-domain-detail",
					component: () =>
						import("@/pages/function-domains/FunctionDomainDetailPage.vue"),
				},
				{
					path: "function-policies",
					name: "function-policies",
					component: () =>
						import("@/pages/function-policies/FunctionPolicyListPage.vue"),
				},
				{
					path: "function-policies/:owner",
					name: "function-policy-detail",
					component: () =>
						import("@/pages/function-policies/FunctionPolicyDetailPage.vue"),
				},
				// Events
				{
					path: "events",
					name: "events",
					component: () => import("@/pages/events/EventListPage.vue"),
				},
				{
					path: "events/:id",
					name: "event-detail",
					component: () => import("@/pages/events/EventListPage.vue"),
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
				},
				{
					path: "processes/create",
					name: "process-create",
					component: () =>
						import("@/pages/processes/ProcessCreatePage.vue"),
				},
				{
					path: "processes/:id",
					name: "process-detail",
					component: () =>
						import("@/pages/processes/ProcessDetailPage.vue"),
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
