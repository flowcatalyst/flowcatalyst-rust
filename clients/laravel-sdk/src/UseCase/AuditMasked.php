<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * A command that declares top-level fields its audit row must mask even
 * though the name rule would keep them (e.g. a config `value` that is only
 * secret when a sibling `valueType` says so). {@see OutboxUnitOfWork} reads
 * the declaration when the command it audits implements this interface; see
 * {@see \FlowCatalyst\Outbox\AuditRedaction} for the rule itself.
 */
interface AuditMasked
{
    /**
     * @return list<string> Top-level field names, as they appear in the
     *   command's audit document.
     */
    public function auditMaskedFields(): array;
}
