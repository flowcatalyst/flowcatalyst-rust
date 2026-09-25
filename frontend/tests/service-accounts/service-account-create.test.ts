// @vitest-environment jsdom
/**
 * Owner ruling: a new service account gets no application access. Go's
 * create drawer carries it (flowcatalyst-go a8ff165): the "Access to all
 * applications" toggle is off by default, so `allApplications` is not sent;
 * switched on, the request asks for every application. Ported from Rust's
 * test of its old full-page form.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import PrimeVue from "primevue/config";
import ConfirmationService from "primevue/confirmationservice";
import Tooltip from "primevue/tooltip";

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

const api = vi.hoisted(() => ({ create: vi.fn() }));
vi.mock("@/api/service-accounts", () => ({ serviceAccountsApi: api }));
vi.mock("@/api/clients", () => ({
	clientsApi: { list: vi.fn().mockResolvedValue({ clients: [] }) },
}));
vi.mock("@/composables/useDrawerRoute", () => ({
	useDrawerRoute: () => ({ goToList: vi.fn(), replaceToDetail: vi.fn() }),
	confirmDiscardChanges: vi.fn(),
}));

const created = {
	serviceAccount: { id: "sa_1" },
	principalId: "prn_1",
	oauth: { clientId: "cid", clientSecret: "cs" },
	webhook: { authToken: "at", signingSecret: "ss" },
};

async function mountDrawer() {
	const { default: Drawer } = await import(
		"@/pages/service-accounts/ServiceAccountCreateDrawer.vue"
	);
	const wrapper = mount(Drawer, {
		attachTo: document.body,
		global: {
			plugins: [PrimeVue, ConfirmationService],
			directives: { tooltip: Tooltip },
			stubs: { teleport: true },
		},
	});
	await flushPromises();
	await wrapper.find('input[placeholder="My Service Account"]').setValue("Billing Sync");
	await wrapper.find('input[placeholder="my-service-account"]').setValue("billing-sync");
	return wrapper;
}

async function submit(wrapper: Awaited<ReturnType<typeof mountDrawer>>) {
	const button = wrapper
		.findAll("button")
		.find((b) => b.text().includes("Create Service Account"));
	expect(button).toBeDefined();
	await button!.trigger("click");
	await flushPromises();
}

beforeEach(() => {
	setActivePinia(createPinia());
	api.create.mockReset();
	api.create.mockResolvedValue(created);
});

describe("ServiceAccountCreateDrawer application access", () => {
	it("is off by default and does not send allApplications", async () => {
		const wrapper = await mountDrawer();
		const toggle = wrapper.find<HTMLInputElement>("input#createAllApplications");
		expect(toggle.exists()).toBe(true);
		expect(toggle.element.checked).toBe(false);

		await submit(wrapper);
		expect(api.create).toHaveBeenCalledTimes(1);
		const request = api.create.mock.calls[0]![0];
		expect(request.code).toBe("billing-sync");
		expect(request.allApplications).toBeUndefined();
		wrapper.unmount();
	});

	it("sends allApplications: true when the toggle is on", async () => {
		const wrapper = await mountDrawer();
		await wrapper.find("input#createAllApplications").setValue(true);

		await submit(wrapper);
		expect(api.create).toHaveBeenCalledTimes(1);
		expect(api.create.mock.calls[0]![0].allApplications).toBe(true);
		wrapper.unmount();
	});
});
