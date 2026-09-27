/**
 * A `globalThis.fetch` stand-in for resource tests: it records every request
 * and answers from a route table keyed by `"METHOD /path"` (path decoded, no
 * query string).
 * Unrouted requests answer 404 so a wrong path fails the test loudly.
 */

import { FlowCatalystClient } from "../../src/index.js";

export interface RecordedRequest {
	method: string;
	path: string;
	query: URLSearchParams;
	/** Parsed JSON body, or undefined when the request had none. */
	body: unknown;
	authorization: string | null;
}

export interface MockReply {
	status?: number;
	body?: unknown;
}

export type Routes = Record<string, MockReply | ((req: RecordedRequest) => MockReply)>;

export interface MockFetch {
	requests: RecordedRequest[];
	client: FlowCatalystClient;
	restore(): void;
}

export const BASE_URL = "http://platform.test";
export const ROUTER_URL = "http://router.test";

export function mockFetch(routes: Routes): MockFetch {
	const original = globalThis.fetch;
	const requests: RecordedRequest[] = [];

	globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
		const request = new Request(input, init);
		const url = new URL(request.url);
		const text = await request.text();
		const recorded: RecordedRequest = {
			method: request.method,
			path: decodeURIComponent(url.pathname),
			query: url.searchParams,
			body: text ? JSON.parse(text) : undefined,
			authorization: request.headers.get("authorization"),
		};
		requests.push(recorded);

		const route = routes[`${recorded.method} ${recorded.path}`];
		if (!route) {
			return new Response(JSON.stringify({ detail: "no route" }), {
				status: 404,
				headers: { "content-type": "application/json" },
			});
		}
		const reply = typeof route === "function" ? route(recorded) : route;
		const status = reply.status ?? 200;
		if (status === 204) {
			return new Response(null, { status: 204 });
		}
		return new Response(JSON.stringify(reply.body ?? {}), {
			status,
			headers: { "content-type": "application/json" },
		});
	}) as typeof fetch;

	const client = new FlowCatalystClient({
		baseUrl: BASE_URL,
		routerBaseUrl: ROUTER_URL,
		accessToken: "test-token",
		retryAttempts: 0,
	});

	return {
		requests,
		client,
		restore() {
			globalThis.fetch = original;
		},
	};
}
