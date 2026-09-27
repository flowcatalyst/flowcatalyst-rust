// @vitest-environment jsdom
/**
 * The dispatch-job drawer shows what the platform now answers (Go's
 * migration 057): the job's descriptor as the drawer title and in the Job
 * column, and its metadata as "Additional data". A job without either falls
 * back to its code and shows no "Additional data" block.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { defineComponent, h, ref } from "vue";
import { flushPromises, mount } from "@vue/test-utils";
import PrimeVue from "primevue/config";
import Tooltip from "primevue/tooltip";
import type { DispatchJobDetail } from "@/api/dispatch-jobs";

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

const api = vi.hoisted(() => ({
	get: vi.fn(),
	attempts: vi.fn(),
	sign: vi.fn(),
	requeue: vi.fn(),
}));

vi.mock("@/api/dispatch-jobs", () => ({ dispatchJobsApi: api }));
vi.mock("@/composables/useDrawerRoute", () => ({
	useDrawerRoute: () => ({ id: ref("djb0000000001"), goToList: vi.fn() }),
}));
// The drawer chrome (PrimeVue Drawer, route guards) is not under test: a
// plain wrapper showing the title and the body.
vi.mock("@/components/drawer/EntityDrawer.vue", () => ({
	default: defineComponent({
		props: { title: { type: String, default: "" } },
		setup(props, { slots }) {
			return () =>
				h("div", [h("h2", { class: "drawer-title" }, props.title), slots.default?.()]);
		},
	}),
}));

import DispatchJobDetailDrawer from "@/pages/dispatch-jobs/DispatchJobDetailDrawer.vue";

function job(overrides: Partial<DispatchJobDetail> = {}): DispatchJobDetail {
	return {
		id: "djb0000000001",
		kind: "EVENT",
		code: "value:iam:user:logged-in",
		targetUrl: "https://subscriber.test/hook",
		protocol: "HTTP_WEBHOOK",
		payloadContentType: "application/json",
		dataOnly: false,
		mode: "IMMEDIATE",
		sequence: 99,
		timeoutSeconds: 30,
		maxRetries: 3,
		retryStrategy: "exponential",
		status: "PENDING",
		attemptCount: 0,
		createdAt: "2026-09-27T00:00:00Z",
		updatedAt: "2026-09-27T00:00:00Z",
		...overrides,
	} as DispatchJobDetail;
}

async function mountDrawer() {
	const wrapper = mount(DispatchJobDetailDrawer, {
		global: {
			plugins: [PrimeVue],
			directives: { tooltip: Tooltip },
		},
	});
	await flushPromises();
	return wrapper;
}

beforeEach(() => {
	vi.clearAllMocks();
	api.attempts.mockResolvedValue([]);
});

describe("dispatch-job drawer", () => {
	it("titles the job by its descriptor and lists its metadata", async () => {
		api.get.mockResolvedValue(
			job({
				descriptor: "Notify Value of user logins",
				metadata: [
					{ key: "userId", value: "u1" },
					{ key: "tenant", value: "acme" },
				],
			}),
		);
		const wrapper = await mountDrawer();

		expect(wrapper.find(".drawer-title").text()).toBe("Notify Value of user logins");
		expect(wrapper.text()).toContain("Descriptor");
		expect(wrapper.text()).toContain("Additional data");
		const items = wrapper.findAll(".kv-item").map((i) => i.text());
		expect(items).toEqual(["userIdu1", "tenantacme"]);
	});

	it("falls back to the code, with no additional data, for a legacy job", async () => {
		api.get.mockResolvedValue(job());
		const wrapper = await mountDrawer();

		expect(wrapper.find(".drawer-title").text()).toBe("value:iam:user:logged-in");
		expect(wrapper.text()).not.toContain("Additional data");
		expect(wrapper.findAll(".kv-item")).toHaveLength(0);
	});
});
