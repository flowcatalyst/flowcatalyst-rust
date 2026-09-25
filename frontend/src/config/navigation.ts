export interface NavItem {
	label: string;
	icon: string;
	route?: string;
	children?: NavItem[];
	expanded?: boolean;
}

export interface NavGroup {
	label: string;
	items: NavItem[];
}

export const NAVIGATION_CONFIG: NavGroup[] = [
	{
		label: "Overview",
		items: [
			{
				label: "Dashboard",
				icon: "pi pi-home",
				route: "/dashboard",
			},
		],
	},
	{
		label: "Identity & Access",
		items: [
			{
				label: "User Management",
				icon: "pi pi-users",
				route: "/users",
			},
			{
				label: "Service Accounts",
				icon: "pi pi-server",
				route: "/identity/service-accounts",
			},
			{
				label: "Identity Providers",
				icon: "pi pi-id-card",
				route: "/authentication/identity-providers",
			},
			{
				label: "Email Domains",
				icon: "pi pi-envelope",
				route: "/authentication/email-domain-mappings",
			},
			{
				label: "OAuth Clients",
				icon: "pi pi-key",
				route: "/authentication/oauth-clients",
			},
			{
				label: "Roles",
				icon: "pi pi-shield",
				route: "/authorization/roles",
			},
			{
				label: "Permissions",
				icon: "pi pi-lock",
				route: "/authorization/permissions",
			},
		],
	},
	{
		label: "Platform",
		items: [
			{
				label: "Applications",
				icon: "pi pi-th-large",
				route: "/applications",
			},
			{
				label: "Clients",
				icon: "pi pi-building",
				route: "/clients",
			},
			{
				label: "CORS Origins",
				icon: "pi pi-link",
				route: "/platform/cors",
			},
			{
				label: "Audit Log",
				icon: "pi pi-history",
				route: "/platform/audit-log",
			},
			{
				label: "Login Attempts",
				icon: "pi pi-sign-in",
				route: "/platform/login-attempts",
			},
			{
				label: "Settings",
				icon: "pi pi-cog",
				expanded: false,
				children: [
					{
						label: "Theme",
						icon: "pi pi-palette",
						route: "/platform/settings/theme",
					},
				],
			},
			{
				label: "Debug",
				icon: "pi pi-wrench",
				expanded: false,
				children: [
					{
						label: "Raw Events",
						icon: "pi pi-database",
						route: "/platform/debug/events",
					},
					{
						label: "Raw Dispatch Jobs",
						icon: "pi pi-database",
						route: "/platform/debug/dispatch-jobs",
					},
				],
			},
		],
	},
	{
		label: "Messaging",
		items: [
			{
				label: "Events",
				icon: "pi pi-inbox",
				route: "/events",
			},
			{
				label: "Event Types",
				icon: "pi pi-bolt",
				route: "/event-types",
			},
			{
				label: "Subscriptions",
				icon: "pi pi-bell",
				route: "/subscriptions",
			},
			{
				label: "Connections",
				icon: "pi pi-link",
				route: "/connections",
			},
			{
				label: "Dispatch Pools",
				icon: "pi pi-database",
				route: "/dispatch-pools",
			},
			{
				label: "Dispatch Jobs",
				icon: "pi pi-send",
				route: "/dispatch-jobs",
			},
			{
				label: "Scheduled Jobs",
				icon: "pi pi-clock",
				route: "/scheduled-jobs",
			},
		],
	},
	{
		label: "Functions",
		items: [
			{
				label: "Functions",
				icon: "pi pi-code",
				route: "/functions",
			},
			{
				label: "Function Domains",
				icon: "pi pi-globe",
				route: "/function-domains",
			},
			{
				label: "Function Policies",
				icon: "pi pi-shield",
				route: "/function-policies",
			},
		],
	},
	{
		label: "Developer",
		items: [
			{
				label: "Applications",
				icon: "pi pi-book",
				route: "/developer",
			},
			{
				label: "Processes",
				icon: "pi pi-sitemap",
				route: "/processes",
			},
		],
	},
];
