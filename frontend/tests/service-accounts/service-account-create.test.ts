// @vitest-environment jsdom
/**
 * The create-service-account form's "All applications" toggle: off by default,
 * so a new account is linked to no application and `allApplications` is not
 * sent; on, the request asks for every application.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import PrimeVue from "primevue/config";
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
vi.mock("vue-router", () => ({ useRouter: () => ({ push: vi.fn() }) }));

const created = {
	serviceAccount: { id: "sa_1" },
	principalId: "prn_1",
	oauth: { clientId: "cid", clientSecret: "cs" },
	webhook: { authToken: "at", signingSecret: "ss" },
};

async function mountPage() {
	const { default: Page } = await import(
		"@/pages/service-accounts/ServiceAccountCreatePage.vue"
	);
	const wrapper = mount(Page, {
		global: {
			plugins: [PrimeVue],
			directives: { tooltip: Tooltip },
			stubs: { teleport: true },
		},
	});
	await flushPromises();
	await wrapper.find("input#name").setValue("Billing Sync");
	await wrapper.find("input#code").setValue("billing-sync");
	return wrapper;
}

async function submit(wrapper: Awaited<ReturnType<typeof mountPage>>) {
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

describe("ServiceAccountCreatePage application access", () => {
	it("is off by default and does not send allApplications", async () => {
		const wrapper = await mountPage();
		const toggle = wrapper.find<HTMLInputElement>("input#allApplications");
		expect(toggle.exists()).toBe(true);
		expect(toggle.element.checked).toBe(false);

		await submit(wrapper);
		expect(api.create).toHaveBeenCalledTimes(1);
		const request = api.create.mock.calls[0]![0];
		expect(request.code).toBe("billing-sync");
		expect(request.allApplications).toBeUndefined();
	});

	it("sends allApplications: true when the toggle is on", async () => {
		const wrapper = await mountPage();
		await wrapper.find("input#allApplications").setValue(true);

		await submit(wrapper);
		expect(api.create).toHaveBeenCalledTimes(1);
		expect(api.create.mock.calls[0]![0].allApplications).toBe(true);
	});
});
