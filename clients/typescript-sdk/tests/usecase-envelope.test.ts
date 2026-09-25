import { test } from "node:test";
import assert from "node:assert/strict";

import { usecase } from "../src/index.js";
import type { OutboxDriver, OutboxMessage } from "../src/outbox/index.js";

// ─── In-memory harness ───────────────────────────────────────────────────────
//
// A transaction is a buffer of deferred commit closures; insert/persist queue
// onto it; withTransaction flushes them on success and discards them on throw —
// modelling all-or-nothing atomicity without a real database.

type Tx = { ops: Array<() => void> };

class MemoryDriver implements OutboxDriver {
	committed: OutboxMessage[] = [];
	failEventInsert = false;

	async insert(message: OutboxMessage, tx?: unknown): Promise<void> {
		if (this.failEventInsert && message.type === "EVENT") {
			throw new Error("simulated outbox insert failure");
		}
		if (tx) (tx as Tx).ops.push(() => this.committed.push(message));
		else this.committed.push(message);
	}

	async insertBatch(messages: OutboxMessage[], tx?: unknown): Promise<void> {
		for (const m of messages) await this.insert(m, tx);
	}

	async withTransaction<T>(callback: (tx: unknown) => Promise<T>): Promise<T> {
		const tx: Tx = { ops: [] };
		const result = await callback(tx); // throw => ops discarded (rollback)
		for (const op of tx.ops) op(); // commit
		return result;
	}
}

interface Order extends usecase.Aggregate {
	readonly id: string;
	readonly totalCents: number;
}

class MemoryOrderRepo implements usecase.Repo<Order> {
	saved: Order[] = [];
	async persist(order: Order, tx: unknown): Promise<void> {
		(tx as Tx).ops.push(() => this.saved.push(order));
	}
	async delete(order: Order, tx: unknown): Promise<void> {
		(tx as Tx).ops.push(() => {
			this.saved = this.saved.filter((o) => o.id !== order.id);
		});
	}
}

class OrderPlaced extends usecase.BaseDomainEvent<{ orderId: string; totalCents: number }> {
	constructor(ctx: usecase.ExecutionContext, data: { orderId: string; totalCents: number }) {
		super(
			{
				eventType: "shop:orders:order:placed",
				specVersion: "1.0",
				source: "shop:orders",
				subject: usecase.DomainEvent.subject("orders", "order", data.orderId),
				messageGroup: usecase.DomainEvent.messageGroup("orders", "order", data.orderId),
			},
			ctx,
			data,
		);
	}
}

interface PlaceOrderCommand extends usecase.Command {
	totalCents: number;
}

// Records phase invocation order so the short-circuit tests can prove that a
// failing phase stops the ones after it.
function placeOrder(repo: MemoryOrderRepo, calls: string[]): usecase.Operation<PlaceOrderCommand, OrderPlaced> {
	return {
		name: "PlaceOrder",
		validate(cmd) {
			calls.push("validate");
			return cmd.totalCents > 0
				? null
				: usecase.UseCaseError.validation("TOTAL_INVALID", "totalCents must be positive");
		},
		authorize(_cmd, ctx) {
			calls.push("authorize");
			return ctx.principalId
				? null
				: usecase.UseCaseError.authorization("NO_PRINCIPAL", "no acting principal");
		},
		async execute(cmd, ctx) {
			calls.push("execute");
			const order: Order = { id: "ord_envelope_1", totalCents: cmd.totalCents };
			const event = new OrderPlaced(ctx, { orderId: order.id, totalCents: order.totalCents });
			return usecase.Plan.save(order, repo, event);
		},
	};
}

const ctx = () => usecase.ExecutionContext.create("prn_test");

// ─── Tests ───────────────────────────────────────────────────────────────────

test("run: happy path persists the aggregate AND writes one EVENT row, atomically", async () => {
	const driver = new MemoryDriver();
	const repo = new MemoryOrderRepo();
	const calls: string[] = [];
	const uow = usecase.OutboxUnitOfWork.fromDriver(driver, "cli_test");

	const result = await usecase.run(uow, placeOrder(repo, calls), { totalCents: 1500 }, ctx());

	assert.ok(usecase.Result.isSuccess(result), "expected success");
	assert.deepEqual(calls, ["validate", "authorize", "execute"], "phases run in order");
	assert.equal(repo.saved.length, 1, "aggregate persisted");
	assert.equal(repo.saved[0]?.id, "ord_envelope_1");
	assert.equal(driver.committed.length, 1, "exactly one outbox row");
	assert.equal(driver.committed[0]?.type, "EVENT");
	assert.equal(driver.committed[0]?.status, 0, "PENDING");
	const payload = JSON.parse(driver.committed[0]!.payload);
	assert.equal(payload.type, "shop:orders:order:placed");
});

test("run: outbox payload is byte-identical to the legacy commit() path (wire parity)", async () => {
	const c = ctx();
	const order: Order = { id: "ord_parity_1", totalCents: 999 };
	const event = new OrderPlaced(c, { orderId: order.id, totalCents: order.totalCents });
	const command = { totalCents: 999 };

	// Same event instance through both paths → payloads must match exactly.
	const viaEnvelope = new MemoryDriver();
	const repo = new MemoryOrderRepo();
	await usecase.run(
		usecase.OutboxUnitOfWork.fromDriver(viaEnvelope, "cli_test"),
		{
			authorize: usecase.publicAuthorize,
			async execute() {
				return usecase.Plan.save(order, repo, event);
			},
		} satisfies usecase.Operation<PlaceOrderCommand, OrderPlaced>,
		command,
		c,
	);

	const viaLegacy = new MemoryDriver();
	await usecase.OutboxUnitOfWork.fromDriver(viaLegacy, "cli_test").commit(event, command);

	assert.equal(viaEnvelope.committed.length, 1);
	assert.equal(viaLegacy.committed.length, 1);
	assert.deepEqual(
		JSON.parse(viaEnvelope.committed[0]!.payload),
		JSON.parse(viaLegacy.committed[0]!.payload),
		"the envelope must not change the outbox event payload",
	);
});

test("run: validation failure short-circuits before authorize/execute and writes nothing", async () => {
	const driver = new MemoryDriver();
	const repo = new MemoryOrderRepo();
	const calls: string[] = [];

	const result = await usecase.run(
		usecase.OutboxUnitOfWork.fromDriver(driver, "cli_test"),
		placeOrder(repo, calls),
		{ totalCents: 0 },
		ctx(),
	);

	assert.ok(usecase.Result.isFailure(result));
	assert.equal(usecase.Result.isFailure(result) && result.error.code, "TOTAL_INVALID");
	assert.deepEqual(calls, ["validate"], "authorize/execute must not run");
	assert.equal(driver.committed.length, 0);
	assert.equal(repo.saved.length, 0);
});

test("run: authorization failure short-circuits before execute and writes nothing", async () => {
	const driver = new MemoryDriver();
	const repo = new MemoryOrderRepo();
	const calls: string[] = [];
	// principalId "" fails the authorize check.
	const noPrincipal = usecase.ExecutionContext.create("");

	const result = await usecase.run(
		usecase.OutboxUnitOfWork.fromDriver(driver, "cli_test"),
		placeOrder(repo, calls),
		{ totalCents: 1500 },
		noPrincipal,
	);

	assert.ok(usecase.Result.isFailure(result));
	assert.equal(usecase.Result.isFailure(result) && result.error.code, "NO_PRINCIPAL");
	assert.deepEqual(calls, ["validate", "authorize"], "execute must not run");
	assert.equal(driver.committed.length, 0);
});

test("run: execute returning a UseCaseError fails without touching the outbox", async () => {
	const driver = new MemoryDriver();

	const result = await usecase.run(
		usecase.OutboxUnitOfWork.fromDriver(driver, "cli_test"),
		{
			authorize: usecase.publicAuthorize,
			async execute() {
				return usecase.UseCaseError.notFound("ORDER_NOT_FOUND", "no such order");
			},
		} satisfies usecase.Operation<PlaceOrderCommand, OrderPlaced>,
		{ totalCents: 1 },
		ctx(),
	);

	assert.ok(usecase.Result.isFailure(result));
	assert.equal(usecase.Result.isFailure(result) && result.error.code, "ORDER_NOT_FOUND");
	assert.equal(driver.committed.length, 0);
});

test("run: Plan.emit writes the event with no aggregate persist", async () => {
	const driver = new MemoryDriver();

	const result = await usecase.run(
		usecase.OutboxUnitOfWork.fromDriver(driver, "cli_test"),
		{
			authorize: usecase.publicAuthorize,
			async execute(_cmd, c) {
				return usecase.Plan.emit(new OrderPlaced(c, { orderId: "ord_emit_1", totalCents: 1 }));
			},
		} satisfies usecase.Operation<PlaceOrderCommand, OrderPlaced>,
		{ totalCents: 1 },
		ctx(),
	);

	assert.ok(usecase.Result.isSuccess(result));
	assert.equal(driver.committed.length, 1);
	assert.equal(driver.committed[0]?.type, "EVENT");
});

test("run: a failed event write rolls back the aggregate persist (atomicity)", async () => {
	const driver = new MemoryDriver();
	driver.failEventInsert = true; // the event insert throws inside the tx
	const repo = new MemoryOrderRepo();

	const result = await usecase.run(
		usecase.OutboxUnitOfWork.fromDriver(driver, "cli_test"),
		placeOrder(repo, []),
		{ totalCents: 1500 },
		ctx(),
	);

	assert.ok(usecase.Result.isFailure(result), "the write must fail");
	assert.equal(driver.committed.length, 0, "no event row committed");
	assert.equal(repo.saved.length, 0, "the aggregate persist must roll back with the event");
});

test("run: outbox carries clientCode (event) + applicationCode/clientCode (audit) when configured", async () => {
	const driver = new MemoryDriver();
	const repo = new MemoryOrderRepo();
	const uow = usecase.OutboxUnitOfWork.fromDriver(driver, "cli_test", {
		auditEnabled: true,
		applicationCode: "shop",
		clientCode: "acme",
	});

	const result = await usecase.run(uow, placeOrder(repo, []), { totalCents: 100 }, ctx());
	assert.ok(usecase.Result.isSuccess(result), "expected success");

	const event = driver.committed.find((m) => m.type === "EVENT");
	const audit = driver.committed.find((m) => m.type === "AUDIT_LOG");
	assert.ok(event && audit, "wrote one event + one audit row");
	const ep = JSON.parse(event!.payload);
	const ap = JSON.parse(audit!.payload);
	assert.equal(ep.clientCode, "acme", "event carries clientCode");
	assert.equal(ap.applicationCode, "shop", "audit carries applicationCode");
	assert.equal(ap.clientCode, "acme", "audit carries clientCode");
});

test("run: linkage codes are omitted from the payload when not configured", async () => {
	const driver = new MemoryDriver();
	const repo = new MemoryOrderRepo();
	const uow = usecase.OutboxUnitOfWork.fromDriver(driver, "cli_test", { auditEnabled: true });

	await usecase.run(uow, placeOrder(repo, []), { totalCents: 100 }, ctx());

	const ep = JSON.parse(driver.committed.find((m) => m.type === "EVENT")!.payload);
	const ap = JSON.parse(driver.committed.find((m) => m.type === "AUDIT_LOG")!.payload);
	assert.equal(ep.clientCode, undefined);
	assert.equal(ap.applicationCode, undefined);
	assert.equal(ap.clientCode, undefined);
});
