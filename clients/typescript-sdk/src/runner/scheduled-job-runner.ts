/**
 * ScheduledJobRunner — handler registry + envelope dispatch + lock + optional
 * completion callback. Mount the `process()` method on whatever HTTP framework
 * you use (Express, Fastify, Hono, raw `Bun.serve`, etc.) at the URL you set
 * as `targetUrl` on the job definition.
 *
 * Two outputs from `process()`:
 *   • The HTTP response (always 202 Accepted on a recognised envelope).
 *   • A background promise (started but not awaited): the actual handler
 *     execution + completion callback. The platform expects 202 within the
 *     dispatcher's `http_timeout` (default 10s) — your handler should run
 *     async, not block the HTTP response.
 *
 * The runner enforces concurrency via an injected `LockProvider`. The
 * platform echoes each job definition's `concurrent` attribute in the
 * envelope, and by default the runner locks exactly when the definition asks
 * for it (`concurrent: false`, the definition default); the lock-key defaults
 * to `scheduled-job:{jobCode}`, and contended fires return immediately
 * without invoking the handler. The default `NoOpLockProvider` cannot
 * actually block overlapping fires — wire a Redis/Postgres provider (a
 * one-time warning is logged when a non-concurrent fire runs without one).
 */

import type { ScheduledJobsResource } from "../resources/scheduled-jobs.js";
import { type LockProvider, NoOpLockProvider } from "./lock-provider.js";

/** Envelope shape POSTed by the platform dispatcher. */
export interface ScheduledJobEnvelope {
	jobId: string;
	jobCode: string;
	instanceId: string;
	scheduledFor?: string;
	firedAt: string;
	triggerKind: "CRON" | "MANUAL";
	correlationId?: string;
	payload?: unknown;
	tracksCompletion: boolean;
	timeoutSeconds?: number;
	/**
	 * Echo of the job definition's `concurrent` attribute. Absent on older
	 * platforms — treated as `false` (lock), the safe direction.
	 */
	concurrent?: boolean;
}

/** Context passed to user handlers. */
export interface HandlerContext {
	envelope: ScheduledJobEnvelope;
	/** Append a structured log entry to this instance. Best-effort. */
	log: (
		message: string,
		opts?: { level?: "DEBUG" | "INFO" | "WARN" | "ERROR"; metadata?: unknown },
	) => Promise<void>;
}

/** User handler. Resolve = success; throw / reject = failure. */
export type Handler = (ctx: HandlerContext) => Promise<unknown>;

export interface RunnerOptions {
	/**
	 * Lock provider for `concurrent: false` jobs. Default `NoOpLockProvider`,
	 * which never blocks — wire `RedisLockProvider`/`PgLockProvider` for real
	 * overlap protection across replicas.
	 */
	lockProvider?: LockProvider;
	/**
	 * Lock key derivation. Default: `scheduled-job:{jobCode}`. Override if you
	 * want per-(job + payload-hash) locking, etc.
	 */
	lockKey?: (envelope: ScheduledJobEnvelope) => string;
	/**
	 * Whether to enforce locking. Default: follow the envelope's `concurrent`
	 * attribute (lock unless the job definition allows overlap). An explicit
	 * true/false overrides the attribute in both directions.
	 */
	enforceLock?: boolean;
	/** Lock TTL in ms. Default 10 minutes. */
	lockTtlMs?: number;
	/** Hook fired on every uncaught handler error (after completion is reported). */
	onError?: (err: unknown, envelope: ScheduledJobEnvelope) => void;
}

export type RunResult =
	| { ok: true; status: 202; bodyJson: { ok: true } }
	| { ok: false; status: 400 | 404; bodyJson: { error: string } };

export class ScheduledJobRunner {
	private readonly handlers: Map<string, Handler> = new Map();
	private readonly resource: ScheduledJobsResource;
	private readonly lockProvider: LockProvider;
	private readonly lockKey: (e: ScheduledJobEnvelope) => string;
	private readonly enforceLock: boolean | undefined;
	private readonly lockTtlMs: number;
	private readonly onError?: (err: unknown, e: ScheduledJobEnvelope) => void;
	private warnedNoOpLock = false;

	constructor(
		_client: unknown,
		resource: ScheduledJobsResource,
		options: RunnerOptions = {},
	) {
		this.resource = resource;
		this.lockProvider = options.lockProvider ?? new NoOpLockProvider();
		this.lockKey =
			options.lockKey ?? ((e) => `scheduled-job:${e.jobCode}`);
		this.enforceLock = options.enforceLock;
		this.lockTtlMs = options.lockTtlMs ?? 10 * 60 * 1000;
		this.onError = options.onError;
	}

	/** Register a handler keyed by the job's `code`. */
	handler(code: string, fn: Handler): this {
		this.handlers.set(code, fn);
		return this;
	}

	/** Convenience: list registered codes (for diagnostics). */
	registeredCodes(): string[] {
		return Array.from(this.handlers.keys());
	}

	/**
	 * Process an inbound platform → SDK firing. Validates the envelope,
	 * acquires the lock, kicks off the handler in the background, and
	 * returns 202 immediately. The actual handler execution + completion
	 * callback continues asynchronously.
	 */
	async process(envelope: unknown): Promise<RunResult> {
		const validated = validateEnvelope(envelope);
		if (!validated.ok) {
			return { ok: false, status: 400, bodyJson: { error: validated.error } };
		}
		const env = validated.envelope;

		const handler = this.handlers.get(env.jobCode);
		if (!handler) {
			return {
				ok: false,
				status: 404,
				bodyJson: { error: `No handler registered for code '${env.jobCode}'` },
			};
		}

		// Spawn but don't await — the platform expects 202 within ~10s. The
		// runner promises only "I have it" via 202; the actual work runs async.
		void this.runInBackground(env, handler);

		return { ok: true, status: 202, bodyJson: { ok: true } };
	}

	private async runInBackground(
		envelope: ScheduledJobEnvelope,
		handler: Handler,
	): Promise<void> {
		const wantLock = this.enforceLock ?? !(envelope.concurrent ?? false);
		let lock: { release: () => Promise<void> } | null = null;
		try {
			if (wantLock) {
				if (
					this.lockProvider instanceof NoOpLockProvider &&
					!this.warnedNoOpLock
				) {
					this.warnedNoOpLock = true;
					console.warn(
						`[flowcatalyst] scheduled job '${envelope.jobCode}' wants overlap protection (concurrent: false) but the runner has no real LockProvider — overlapping fires will NOT be blocked. Pass a RedisLockProvider/PgLockProvider in RunnerOptions.lockProvider.`,
					);
				}
				lock = await this.lockProvider.acquire(
					this.lockKey(envelope),
					this.lockTtlMs,
				);
				if (lock === null) {
					// Lock contention — skip this firing. If the job tracks
					// completion, mark as failure with a clear reason; otherwise
					// just no-op (the instance will sit DELIVERED forever).
					if (envelope.tracksCompletion) {
						await this.resource
							.completeInstance(envelope.instanceId, {
								status: "FAILURE",
								result: { skipped: true, reason: "lock-held" },
							})
							.match(
								() => undefined,
								(e) => {
									this.onError?.(e, envelope);
								},
							);
					}
					return;
				}
			}

			const ctx: HandlerContext = {
				envelope,
				log: (message, opts = {}) =>
					this.resource
						.logForInstance(envelope.instanceId, {
							message,
							level: opts.level ?? "INFO",
							metadata: opts.metadata,
						})
						.match(
							() => undefined,
							(e) => {
								this.onError?.(e, envelope);
							},
						),
			};

			let result: unknown;
			let succeeded = true;
			let thrownError: unknown = undefined;
			try {
				result = await handler(ctx);
			} catch (err) {
				succeeded = false;
				thrownError = err;
			}

			if (envelope.tracksCompletion) {
				await this.resource
					.completeInstance(envelope.instanceId, {
						status: succeeded ? "SUCCESS" : "FAILURE",
						result: succeeded
							? sanitiseResult(result)
							: { error: errorMessage(thrownError) },
					})
					.match(
						() => undefined,
						(e) => {
							this.onError?.(e, envelope);
						},
					);
			}

			if (!succeeded) this.onError?.(thrownError, envelope);
		} finally {
			if (lock) {
				try {
					await lock.release();
				} catch (e) {
					this.onError?.(e, envelope);
				}
			}
		}
	}
}

// ── Helpers ────────────────────────────────────────────────────────────────

function validateEnvelope(
	v: unknown,
): { ok: true; envelope: ScheduledJobEnvelope } | { ok: false; error: string } {
	if (!v || typeof v !== "object") {
		return { ok: false, error: "Envelope must be a JSON object" };
	}
	const o = v as Record<string, unknown>;
	const required = ["jobId", "jobCode", "instanceId", "firedAt", "triggerKind"];
	for (const k of required) {
		if (typeof o[k] !== "string") {
			return { ok: false, error: `Envelope missing string field '${k}'` };
		}
	}
	if (typeof o["tracksCompletion"] !== "boolean") {
		return { ok: false, error: "Envelope missing boolean field 'tracksCompletion'" };
	}
	const trigger = o["triggerKind"] as string;
	if (trigger !== "CRON" && trigger !== "MANUAL") {
		return { ok: false, error: `Invalid triggerKind '${trigger}'` };
	}
	return {
		ok: true,
		envelope: {
			jobId: o["jobId"] as string,
			jobCode: o["jobCode"] as string,
			instanceId: o["instanceId"] as string,
			scheduledFor:
				typeof o["scheduledFor"] === "string" ? (o["scheduledFor"] as string) : undefined,
			firedAt: o["firedAt"] as string,
			triggerKind: trigger as "CRON" | "MANUAL",
			correlationId:
				typeof o["correlationId"] === "string" ? (o["correlationId"] as string) : undefined,
			payload: o["payload"],
			tracksCompletion: o["tracksCompletion"] as boolean,
			timeoutSeconds:
				typeof o["timeoutSeconds"] === "number" ? (o["timeoutSeconds"] as number) : undefined,
			concurrent:
				typeof o["concurrent"] === "boolean" ? (o["concurrent"] as boolean) : undefined,
		},
	};
}

function sanitiseResult(v: unknown): unknown {
	// Small payload only — completion_result is JSONB but should not be huge.
	// Cap at ~10KB by JSON-stringifying and substringing if needed.
	try {
		const json = JSON.stringify(v);
		if (json.length > 10_000) {
			return { truncated: true, preview: json.slice(0, 10_000) };
		}
		return v;
	} catch {
		return { unserialisable: true };
	}
}

function errorMessage(e: unknown): string {
	if (e instanceof Error) return e.message;
	if (typeof e === "string") return e;
	try {
		return JSON.stringify(e);
	} catch {
		return "Unknown error";
	}
}
