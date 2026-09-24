<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Outbox;

use FlowCatalyst\Outbox\AuditRedaction;
use FlowCatalyst\Outbox\Contracts\OutboxDriver;
use FlowCatalyst\Outbox\DTOs\CreateAuditLogDto;
use FlowCatalyst\Outbox\OutboxManager;
use FlowCatalyst\UseCase\AuditMasked;
use FlowCatalyst\UseCase\BaseDomainEvent;
use FlowCatalyst\UseCase\ExecutionContext;
use FlowCatalyst\UseCase\OutboxUnitOfWork;
use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\TestCase;

/**
 * Audit logs must never store passwords or secrets (docs/spec/audit-redaction.md).
 * tests/Fixtures/audit-redaction-vectors.json is a byte-identical copy of the
 * canonical docs/spec/audit-redaction-vectors.json — every case here must
 * pass in every SDK and in the platform.
 */
final class AuditRedactionTest extends TestCase
{
    public function test_fixture_copy_is_byte_identical_to_the_canonical_spec_vectors(): void
    {
        $canonicalPath = __DIR__ . '/../../../../../docs/spec/audit-redaction-vectors.json';
        $copyPath = __DIR__ . '/../../Fixtures/audit-redaction-vectors.json';

        $this->assertFileExists($canonicalPath, 'the canonical spec vectors file must exist');
        $this->assertSame(file_get_contents($canonicalPath), file_get_contents($copyPath));
    }

    public function test_redact_never_mutates_its_input(): void
    {
        $input = ['password' => 'hunter2', 'nested' => ['token' => 't']];
        $before = json_encode($input);

        AuditRedaction::redact($input);

        $this->assertSame($before, json_encode($input));
    }

    #[DataProvider('vectorsProvider')]
    public function test_vector(string $name, array $input, array $masked, array $expected): void
    {
        $actual = AuditRedaction::redact($input, $masked);
        $this->assertSame($expected, $actual, "vector '{$name}' did not redact as expected");
    }

    /** @return array<int, array{0: string, 1: array, 2: array, 3: array}> */
    public static function vectorsProvider(): array
    {
        $path = __DIR__ . '/../../Fixtures/audit-redaction-vectors.json';
        $vectors = json_decode(file_get_contents($path), true, flags: JSON_THROW_ON_ERROR);

        $cases = [];
        foreach ($vectors as $vector) {
            $cases[$vector['name']] = [
                $vector['name'],
                $vector['input'],
                $vector['masked'],
                $vector['expected'],
            ];
        }

        return $cases;
    }

    public function test_password_in_operation_data_never_appears_in_the_outbox_payload_string(): void
    {
        $dto = CreateAuditLogDto::create('Principal', 'p_1', 'CREATE')->withOperationData([
            'email' => 'a@b.c',
            'password' => 'hunter2',
            'webhookCredentials' => ['token' => 'tok', 'signingSecret' => 's3'],
        ]);

        $payload = json_encode($dto->toPayload());

        $this->assertStringNotContainsString('hunter2', $payload, 'password value must not appear in the payload');
        $this->assertStringNotContainsString('"tok"', $payload, 'nested token value must not appear in the payload');
        $this->assertStringNotContainsString('"s3"', $payload, 'nested secret value must not appear in the payload');
        $this->assertStringContainsString('a@b.c', $payload, 'non-secret fields must survive redaction');
    }

    public function test_a_secret_inside_a_nested_value_object_is_redacted(): void
    {
        $credentials = new class {
            public string $authType = 'HMAC_SIGNATURE';
            public string $signingSecret = 's3cret';
        };

        $actual = AuditRedaction::redact(['code' => 'sa-1', 'webhookCredentials' => $credentials]);

        $this->assertSame(
            ['code' => 'sa-1', 'webhookCredentials' => ['authType' => 'HMAC_SIGNATURE', 'signingSecret' => '***']],
            $actual,
        );
    }

    public function test_the_outbox_unit_of_work_redacts_and_honours_audit_masked(): void
    {
        $driver = new class implements OutboxDriver {
            /** @var list<array<string, mixed>> */
            public array $messages = [];

            public function insert(array $message): void
            {
                $this->messages[] = $message;
            }

            public function insertBatch(array $messages): void
            {
                foreach ($messages as $message) {
                    $this->messages[] = $message;
                }
            }
        };
        $uow = new OutboxUnitOfWork(new OutboxManager($driver, 'clt_1'), auditEnabled: true);

        foreach (['SECRET', 'PLAIN'] as $valueType) {
            $result = $uow->emitEvent(new AuditTestPropertySet(), new AuditTestSetPropertyCommand('sk_live_123', $valueType));
            $this->assertTrue($result->isSuccess());
        }

        $audits = array_values(array_filter($driver->messages, fn (array $m): bool => $m['type'] === 'AUDIT_LOG'));
        $this->assertCount(2, $audits);
        $data = array_map(
            fn (array $m): array => json_decode(json_decode($m['payload'], true)['operationData'], true),
            $audits,
        );
        $this->assertSame(
            ['property' => 'stripe', 'apiKey' => '***', 'value' => '***', 'valueType' => 'SECRET'],
            $data[0],
        );
        $this->assertSame(
            ['property' => 'stripe', 'apiKey' => '***', 'value' => 'sk_live_123', 'valueType' => 'PLAIN'],
            $data[1],
        );
    }

    public function test_with_operation_data_masks_a_caller_declared_field_the_name_rule_would_keep(): void
    {
        $dto = CreateAuditLogDto::create('PlatformConfig', 'cfg_1', 'SET_PROPERTY')->withOperationData(
            ['property' => 'key', 'value' => 'sk_live_123', 'valueType' => 'SECRET'],
            ['value'],
        );

        $operationData = json_decode($dto->toPayload()['operationData'], true, flags: JSON_THROW_ON_ERROR);

        $this->assertSame('***', $operationData['value']);
        $this->assertSame('SECRET', $operationData['valueType'], 'unmasked sibling fields survive');
    }
}

final class AuditTestPropertySet extends BaseDomainEvent
{
    public function __construct()
    {
        parent::__construct(
            [
                'eventType' => 'shop:config:property:set',
                'specVersion' => '1.0',
                'source' => 'shop:config',
                'subject' => 'config.property.cfg_1',
                'messageGroup' => 'config:property:cfg_1',
            ],
            ExecutionContext::create('prn_1'),
            ['property' => 'stripe'],
        );
    }
}

final class AuditTestSetPropertyCommand implements AuditMasked
{
    public string $property = 'stripe';
    public string $apiKey = 'k';

    public function __construct(
        public string $value,
        public string $valueType,
    ) {}

    public function auditMaskedFields(): array
    {
        return $this->valueType === 'PLAIN' ? [] : ['value'];
    }
}
