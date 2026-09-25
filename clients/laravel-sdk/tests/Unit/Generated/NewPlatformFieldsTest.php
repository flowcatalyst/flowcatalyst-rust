<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Generated;

use FlowCatalyst\Generated\Model\CreateServiceAccountRequest;
use FlowCatalyst\Generated\Model\SyncResultResponse;
use FlowCatalyst\Generated\Model\SyncUsersResponse;
use FlowCatalyst\Generated\Normalizer\CreateServiceAccountRequestNormalizer;
use FlowCatalyst\Generated\Normalizer\SyncResultResponseNormalizer;
use FlowCatalyst\Generated\Normalizer\SyncUsersResponseNormalizer;
use PHPUnit\Framework\TestCase;

/**
 * Fields the platform gained on 2026-09-25, hand-added to the generated models
 * (the published generator's output, see docs/sdks.md): service-account
 * `allApplications` and the principal syncs' `passwordHashIgnored`.
 */
final class NewPlatformFieldsTest extends TestCase
{
    public function testCreateServiceAccountSendsAllApplicationsOnlyWhenSet(): void
    {
        $normalizer = new CreateServiceAccountRequestNormalizer();

        $plain = (new CreateServiceAccountRequest())->setCode('svc')->setName('Svc');
        $this->assertArrayNotHasKey('allApplications', $normalizer->normalize($plain));

        $all = (new CreateServiceAccountRequest())->setCode('svc')->setName('Svc')->setAllApplications(true);
        $this->assertTrue($normalizer->normalize($all)['allApplications']);

        $read = $normalizer->denormalize(['code' => 'svc', 'name' => 'Svc', 'allApplications' => true], CreateServiceAccountRequest::class);
        $this->assertTrue($read->getAllApplications());
    }

    public function testSyncResponsesReadPasswordHashIgnored(): void
    {
        $app = (new SyncResultResponseNormalizer())->denormalize([
            'applicationCode' => 'hr', 'created' => 0, 'updated' => 1, 'deleted' => 0,
            'syncedCodes' => ['a@example.com'], 'passwordHashIgnored' => ['a@example.com'],
        ], SyncResultResponse::class);
        $this->assertSame(['a@example.com'], $app->getPasswordHashIgnored());

        $users = (new SyncUsersResponseNormalizer())->denormalize([
            'created' => 0, 'updated' => 1, 'deleted' => 0,
            'syncedEmails' => ['a@example.com'], 'passwordHashIgnored' => ['a@example.com'],
        ], SyncUsersResponse::class);
        $this->assertSame(['a@example.com'], $users->getPasswordHashIgnored());

        $omitted = (new SyncUsersResponseNormalizer())->denormalize([
            'created' => 1, 'updated' => 0, 'deleted' => 0, 'syncedEmails' => ['b@example.com'],
        ], SyncUsersResponse::class);
        $this->assertNull($omitted->getPasswordHashIgnored());
        $this->assertArrayNotHasKey('passwordHashIgnored', (new SyncUsersResponseNormalizer())->normalize($omitted));
    }
}
