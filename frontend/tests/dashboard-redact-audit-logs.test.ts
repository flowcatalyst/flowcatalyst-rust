// @vitest-environment jsdom
/**
 * docs/spec/audit-redaction.md (Java repo), "Temporary: redact existing rows
 * from the dashboard": the dashboard's "Audit logs" card confirms before it runs,
 * calls the redact-existing route, and shows the returned counts in a
 * toast. This is NOT part of "Sync All" — it must not fire on that click.
 */

import { describe, expect, it, vi, beforeEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import PrimeVue from "primevue/config";
import ConfirmationService from "primevue/confirmationservice";
import ConfirmDialog from "primevue/confirmdialog";
import { onNotification, type Notification } from "@/utils/errorBus";
import { useAuthStore } from "@/stores/auth";

if (typeof window !== "undefined" && !window.matchMedia) {
	window.matchMedia = ((query: string) => ({
		matches: false,
		media: query,
		onchange: null,
		addListener: () => {},
		removeListener: () => {},
		addEventListener: () => {},
		removeEventListener: () => {},
		dispatchEvent: () => false,
	})) as unknown as typeof window.matchMedia;
}

const mocks = vi.hoisted(() => ({
	redactExistingAuditLogs: vi.fn(),
}));

vi.mock("@/api/audit-logs", async (importOriginal) => {
	const actual = await importOriginal<typeof import("@/api/audit-logs")>();
	return { ...actual, redactExistingAuditLogs: mocks.redactExistingAuditLogs };
});

vi.mock("@/api/dashboard", () => ({
	dashboardApi: { stats: vi.fn(async () => Promise.reject(new Error("not needed for this test"))) },
}));

// Neither exercised here — stub so mount() never makes a real network call.
vi.mock("@/api/event-types", () => ({ eventTypesApi: { syncPlatform: vi.fn() } }));
vi.mock("@/api/roles", () => ({ rolesApi: { syncPlatform: vi.fn() } }));
vi.mock("@/api/developer", () => ({ developerApi: { syncPlatformOpenApi: vi.fn() } }));

async function mountDashboard() {
	const { default: DashboardPage } = await import("@/pages/DashboardPage.vue");
	// An anchor super admin: the dashboard shows the audit-log card only to a
	// user who may run it (anchor and the audit-log read permission).
	const pinia = createPinia();
	setActivePinia(pinia);
	useAuthStore().setUser({
		id: "prn_admin",
		email: "admin@example.com",
		name: "Admin",
		clientId: null,
		roles: ["platform:super-admin"],
		permissions: ["platform:*:*:*", "*"],
		scope: "ANCHOR",
	});
	return mount(
		{ components: { DashboardPage, ConfirmDialog }, template: "<div><DashboardPage /><ConfirmDialog /></div>" },
		{
			global: {
				plugins: [pinia, PrimeVue, ConfirmationService],
				stubs: { RouterLink: true },
			},
		},
	);
}

function collectNotifications(): Notification[] {
	const seen: Notification[] = [];
	onNotification((n) => seen.push(n));
	return seen;
}

describe("DashboardPage — temporary audit-log redaction card", () => {
	beforeEach(() => {
		setActivePinia(createPinia());
		mocks.redactExistingAuditLogs.mockReset();
	});

	it("confirms, then calls redact-existing and toasts the returned counts", async () => {
		mocks.redactExistingAuditLogs.mockResolvedValue({ scanned: 42, redacted: 7 });
		const notifications = collectNotifications();

		const wrapper = await mountDashboard();
		await flushPromises();

		const redactButton = wrapper
			.findAll("button")
			.find((b) => b.text().includes("Redact"));
		expect(redactButton, "the dashboard renders a Redact button").toBeTruthy();

		await redactButton!.trigger("click");
		await flushPromises();

		// The confirm dialog is teleported to document.body by PrimeVue.
		expect(mocks.redactExistingAuditLogs).not.toHaveBeenCalled();
		const acceptButton = Array.from(document.querySelectorAll("button")).find((b) =>
			b.textContent?.includes("Redact"),
		);
		expect(acceptButton, "a confirmation dialog with a Redact accept button is shown").toBeTruthy();

		acceptButton!.dispatchEvent(new Event("click", { bubbles: true }));
		await flushPromises();

		expect(mocks.redactExistingAuditLogs).toHaveBeenCalledTimes(1);
		const toasted = notifications.find((n) => n.summary === "Audit Logs Redacted");
		expect(toasted, "a toast with the counts is shown").toBeTruthy();
		expect(toasted!.detail).toContain("7");
		expect(toasted!.detail).toContain("42");
	});

	it("does not call redact-existing without going through the confirmation", async () => {
		mocks.redactExistingAuditLogs.mockResolvedValue({ scanned: 1, redacted: 0 });

		const wrapper = await mountDashboard();
		await flushPromises();

		const redactButton = wrapper
			.findAll("button")
			.find((b) => b.text().includes("Redact"));
		await redactButton!.trigger("click");
		await flushPromises();

		// Mutant: wiring the button straight to the API call, skipping confirm.require,
		// would call the mock here — it must not.
		expect(mocks.redactExistingAuditLogs).not.toHaveBeenCalled();
	});

	it("is not part of Sync All", async () => {
		mocks.redactExistingAuditLogs.mockResolvedValue({ scanned: 1, redacted: 0 });

		const wrapper = await mountDashboard();
		await flushPromises();

		const syncAll = wrapper.findAll("button").find((b) => b.text().includes("Sync All"));
		expect(syncAll, "the dashboard renders Sync All").toBeTruthy();
		await syncAll!.trigger("click");
		await flushPromises();

		expect(mocks.redactExistingAuditLogs).not.toHaveBeenCalled();
	});
});
