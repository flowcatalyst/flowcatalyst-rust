// @vitest-environment jsdom
/**
 * The function detail tabs, mounted with the API module mocked:
 * - Versions: Promote only for READY, Retire never for the live version.
 * - Secrets: a stored value never reaches the DOM, before or after a set.
 * - Publish: the artifact is hashed and uploaded BEFORE publish, and publish
 *   uses the ref the upload returned; a failed upload never publishes and
 *   its code and details are shown.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import PrimeVue from "primevue/config";
import ConfirmationService from "primevue/confirmationservice";
import Tooltip from "primevue/tooltip";
import { ApiError } from "@/api/client";
import { useAuthStore } from "@/stores/auth";
import type { VersionResponse } from "@/api/functions";

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
	listVersions: vi.fn(),
	listAliases: vi.fn(),
	getVersion: vi.fn(),
	getConfig: vi.fn(),
	setConfig: vi.fn(),
	listSecrets: vi.fn(),
	setSecret: vi.fn(),
	deleteSecret: vi.fn(),
	uploadArtifact: vi.fn(),
	publishVersion: vi.fn(),
}));

vi.mock("@/api/functions", () => ({ functionsApi: api }));
vi.mock("vue-router", () => ({ useRouter: () => ({ push: vi.fn() }) }));

function version(v: number, state: VersionResponse["state"], live = false): VersionResponse {
	return {
		id: `fnv_${v}`,
		version: v,
		state,
		digest: `sha256:${String(v).repeat(64)}`,
		artifactRef: `platform://fn_1/${v}`,
		pool: "default",
		warm: false,
		publishedBy: "admin",
		publishedAt: "2026-09-25T00:00:00Z",
		live,
	};
}

function mountOptions() {
	return {
		global: {
			plugins: [PrimeVue, ConfirmationService],
			directives: { tooltip: Tooltip },
			stubs: { RouterLink: true, teleport: true },
		},
	};
}

beforeEach(() => {
	setActivePinia(createPinia());
	useAuthStore().setUser({
		id: "p1",
		email: "admin@acme.test",
		name: "Admin",
		clientId: null,
		roles: ["platform:super-admin"],
		permissions: ["platform:*:*:*"],
	});
	for (const fn of Object.values(api)) fn.mockReset();
});

describe("FunctionVersionsTab", () => {
	it("enables Promote only for READY and Retire never for the live version", async () => {
		api.listVersions.mockResolvedValue([
			version(1, "RETIRED"),
			version(2, "READY", true),
			version(3, "READY"),
			version(4, "PUBLISHED"),
		]);
		api.listAliases.mockResolvedValue([]);
		const { default: FunctionVersionsTab } = await import("@/pages/functions/FunctionVersionsTab.vue");
		const wrapper = mount(FunctionVersionsTab, { props: { address: "a.b.c" }, ...mountOptions() });
		await flushPromises();

		const rows = wrapper.findAll("tbody tr");
		const byVersion = new Map(rows.map((r) => [r.text().match(/v(\d+)/)?.[1], r]));
		const disabled = (v: string, testid: string) =>
			byVersion.get(v)!.find(`[data-testid="${testid}"]`).attributes("disabled") !== undefined;

		// Newest first.
		expect(rows[0]!.text()).toContain("v4");
		expect(disabled("4", "promote-button")).toBe(true);
		expect(disabled("3", "promote-button")).toBe(false);
		expect(disabled("2", "promote-button")).toBe(false);
		expect(disabled("1", "promote-button")).toBe(true);

		expect(disabled("2", "retire-button")).toBe(true);
		expect(disabled("1", "retire-button")).toBe(true);
		expect(disabled("3", "retire-button")).toBe(false);
		expect(disabled("4", "retire-button")).toBe(false);
	});
});

describe("FunctionConfigSecretsTab", () => {
	it("never renders a secret's value, and clears it from the form after a set", async () => {
		api.getConfig.mockResolvedValue({ values: {}, declared: [], missing: [], declaredBy: [] });
		api.listSecrets.mockResolvedValue({
			keys: [],
			declared: ["API_KEY"],
			missing: ["API_KEY"],
			declaredBy: [{ version: 1, keys: ["API_KEY"] }],
		});
		api.setSecret.mockResolvedValue(undefined);
		const { default: Tab } = await import("@/pages/functions/FunctionConfigSecretsTab.vue");
		const wrapper = mount(Tab, { props: { address: "a.b.c", liveVersion: 1 }, ...mountOptions() });
		await flushPromises();

		expect(wrapper.find('[data-testid="settings-missing-banner"]').exists()).toBe(true);
		const row = wrapper.find('[data-testid="secret-row"]');
		expect(row.text()).toContain("not set");

		await row.find('[data-testid="secret-set-button"]').trigger("click");
		const input = wrapper.find('[data-testid="secret-value-input"]');
		await input.setValue("hunter2-super-secret");

		// After the save the server lists the key as set; the value is never returned.
		api.listSecrets.mockResolvedValue({
			keys: [{ key: "API_KEY", updatedAt: "2026-09-25T00:00:00Z", updatedBy: "admin" }],
			declared: ["API_KEY"],
			missing: [],
			declaredBy: [{ version: 1, keys: ["API_KEY"] }],
		});
		await wrapper.find('[data-testid="secret-save-button"]').trigger("click");
		await flushPromises();

		expect(api.setSecret).toHaveBeenCalledWith(
			"a.b.c",
			"API_KEY",
			{ value: "hunter2-super-secret" },
			{ suppressGlobalErrorToast: true },
		);
		expect(wrapper.html()).not.toContain("hunter2-super-secret");
		expect(wrapper.find('[data-testid="secret-value-input"]').exists()).toBe(false);
		expect(wrapper.find('[data-testid="secret-row"]').text()).toContain("set");
		expect(wrapper.find('[data-testid="secret-row"]').text()).not.toContain("not set");
	});

	it("shows the no-app-key 503 as a disabled state", async () => {
		api.getConfig.mockResolvedValue({ values: {}, declared: [], missing: [], declaredBy: [] });
		api.listSecrets.mockRejectedValue(
			new ApiError("FLOWCATALYST_APP_KEY is not configured", 503, "ENCRYPTION_UNCONFIGURED"),
		);
		const { default: Tab } = await import("@/pages/functions/FunctionConfigSecretsTab.vue");
		const wrapper = mount(Tab, { props: { address: "a.b.c" }, ...mountOptions() });
		await flushPromises();
		expect(wrapper.find('[data-testid="secrets-disabled"]').text()).toContain(
			"FLOWCATALYST_APP_KEY is not configured",
		);
	});
});

describe("PublishVersionDialog", () => {
	const MANIFEST = { runtime: "wasm", entrypoint: "wasi_http_incoming_handler" } as const;
	// sha256 of the three bytes 0x01 0x02 0x03.
	const DIGEST = "sha256:039058c6f2c0cb492c533b0a4d14ef77cc0f78abccced5287d84a1a2011cfb81";

	async function mountDialog() {
		const { default: Dialog } = await import("@/pages/functions/PublishVersionDialog.vue");
		const wrapper = mount(Dialog, {
			props: { address: "a.b.c", initialManifest: MANIFEST },
			attachTo: document.body,
			...mountOptions(),
		});
		await flushPromises();
		const input = wrapper.find('[data-testid="publish-artifact-input"]');
		const file = new File([new Uint8Array([1, 2, 3])], "fn.wasm", { type: "application/wasm" });
		Object.defineProperty(input.element, "files", { value: [file] });
		await input.trigger("change");
		return wrapper;
	}

	it("uploads first, then publishes with the ref the upload returned", async () => {
		const order: string[] = [];
		api.uploadArtifact.mockImplementation(async () => {
			order.push("upload");
			return { artifactRef: "platform://fn_1/from-server", digest: DIGEST, bytes: 3 };
		});
		api.publishVersion.mockImplementation(async () => {
			order.push("publish");
			return { id: "fnv_5", version: 5, state: "PUBLISHED", digest: DIGEST };
		});
		const wrapper = await mountDialog();

		await wrapper.find('[data-testid="publish-submit"]').trigger("click");
		// Hashing (crypto.subtle) and File.arrayBuffer are real async work.
		await vi.waitFor(() => expect(wrapper.emitted("published")).toBeTruthy());

		expect(order).toEqual(["upload", "publish"]);
		expect(api.uploadArtifact.mock.calls[0]![0]).toBe("a.b.c");
		expect(api.uploadArtifact.mock.calls[0]![1]).toBe(DIGEST);
		expect(api.publishVersion.mock.calls[0]![1]).toEqual({
			artifactRef: "platform://fn_1/from-server",
			digest: DIGEST,
			manifest: MANIFEST,
			signatureBundle: undefined,
		});
		expect(wrapper.emitted("published")?.[0]?.[0]).toMatchObject({ version: 5 });
		wrapper.unmount();
	});

	it("does not publish after a failed upload, and shows the code and details", async () => {
		api.uploadArtifact.mockRejectedValue(
			new ApiError("digest does not match the body", 400, "DIGEST_MISMATCH", {
				errors: [{ message: "expected sha256:…", location: "path.digest" }],
			}),
		);
		const wrapper = await mountDialog();

		await wrapper.find('[data-testid="publish-submit"]').trigger("click");
		await vi.waitFor(() =>
			expect(wrapper.find('[data-testid="publish-error"]').exists()).toBe(true),
		);

		expect(api.uploadArtifact).toHaveBeenCalledTimes(1);
		expect(api.publishVersion).not.toHaveBeenCalled();
		const error = wrapper.find('[data-testid="publish-error"]');
		expect(error.text()).toContain("DIGEST_MISMATCH");
		expect(error.text()).toContain("path.digest: expected sha256:…");
		wrapper.unmount();
	});
});
