/**
 * UnitOfWork — atomic commit of entity state + domain event (+ optional audit log).
 *
 * Concrete implementations route events to their destination. For this SDK the
 * default is `OutboxUnitOfWork`, which writes events to the local outbox table
 * so the fc-outbox-processor forwards them to the FlowCatalyst platform.
 */
export {};
