<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Client\Resources;

use FlowCatalyst\Client\Auth\UserTokenProvider;
use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\Client\Resources\Applications;
use FlowCatalyst\Client\Resources\AuditLogs;
use FlowCatalyst\Client\Resources\Clients;
use FlowCatalyst\Client\Resources\Connections;
use FlowCatalyst\Client\Resources\DispatchPools;
use FlowCatalyst\Client\Resources\EventTypes;
use FlowCatalyst\Client\Resources\Permissions;
use FlowCatalyst\Client\Resources\Principals;
use FlowCatalyst\Client\Resources\Roles;
use FlowCatalyst\Client\Resources\Router;
use FlowCatalyst\Client\Resources\ScheduledJobs;
use FlowCatalyst\Client\Resources\Subscriptions;
use FlowCatalyst\DTOs\Requests\CreateApplicationRequest;
use FlowCatalyst\DTOs\Requests\CreateEventTypeRequest;
use FlowCatalyst\DTOs\Requests\UpdateApplicationRequest;
use FlowCatalyst\DTOs\Requests\UpdateClientRequest;
use FlowCatalyst\DTOs\Requests\UpdateConnectionRequest;
use FlowCatalyst\DTOs\Requests\UpdateEventTypeRequest;
use FlowCatalyst\Exceptions\FlowCatalystException;
use GuzzleHttp\Client;
use GuzzleHttp\Handler\MockHandler;
use GuzzleHttp\HandlerStack;
use GuzzleHttp\Psr7\Response;
use PHPUnit\Framework\TestCase;

/**
 * The hand-written resources against the Go platform's wire shapes
 * (`frontend/openapi/openapi.json`, Go's `api/openapi.lock.json`): paths,
 * query members, request bodies, and how each response is read.
 */
final class GoApiWireTest extends TestCase
{
    /** @var list<array{method: string, endpoint: string, options: array<string, mixed>}> */
    private array $calls = [];

    /**
     * A client whose `request()` records each call and answers with the next
     * queued response (an empty array stands for a 204).
     *
     * @param list<array<mixed>> $responses
     */
    private function client(array $responses): FlowCatalystClient
    {
        $this->calls = [];
        $client = $this->createStub(FlowCatalystClient::class);
        $client->method('request')->willReturnCallback(
            function (string $method, string $endpoint, array $options = []) use (&$responses) {
                $this->calls[] = ['method' => $method, 'endpoint' => $endpoint, 'options' => $options];

                return array_shift($responses) ?? [];
            },
        );

        return $client;
    }

    /** @return array<string, string> */
    private function query(int $call): array
    {
        $query = parse_url($this->calls[$call]['endpoint'], PHP_URL_QUERY);
        parse_str((string) $query, $params);

        /** @var array<string, string> $params */
        return $params;
    }

    private function path(int $call): string
    {
        return (string) parse_url($this->calls[$call]['endpoint'], PHP_URL_PATH);
    }

    // ── applications ────────────────────────────────────────────────────

    public function test_application_create_returns_the_id(): void
    {
        $apps = new Applications($this->client([['id' => 'app_1']]));

        $id = $apps->create(new CreateApplicationRequest(code: 'orders', name: 'Orders'));

        $this->assertSame('app_1', $id);
        $this->assertSame('POST', $this->calls[0]['method']);
        $this->assertSame('/api/applications', $this->calls[0]['endpoint']);
    }

    public function test_application_update_reads_no_body(): void
    {
        $apps = new Applications($this->client([[]]));

        $apps->update('app_1', new UpdateApplicationRequest(name: 'Orders 2'));

        $this->assertSame('PUT', $this->calls[0]['method']);
        $this->assertSame('/api/applications/app_1', $this->calls[0]['endpoint']);
    }

    public function test_application_list_sends_active_and_type(): void
    {
        $apps = new Applications($this->client([['applications' => [], 'total' => 0]]));

        $apps->list(active: true, type: 'INTEGRATION');

        $this->assertSame(['active' => 'true', 'type' => 'INTEGRATION'], $this->query(0));
    }

    public function test_get_service_account_reads_the_application_then_the_service_account(): void
    {
        $apps = new Applications($this->client([
            ['id' => 'app_1', 'code' => 'orders', 'name' => 'Orders', 'type' => 'APPLICATION', 'active' => true,
                'hasLoginClient' => false, 'serviceAccountId' => 'sa_1', 'createdAt' => 't', 'updatedAt' => 't'],
            ['id' => 'sa_1', 'code' => 'orders-sa', 'name' => 'Orders SA', 'active' => true, 'authType' => 'BEARER',
                'clientIds' => [], 'roles' => [], 'principalId' => 'prn_1', 'oauthClientId' => 'oac_1',
                'createdAt' => 't', 'updatedAt' => 't'],
        ]));

        $sa = $apps->getServiceAccount('app_1');

        $this->assertSame(['GET', 'GET'], array_column($this->calls, 'method'));
        $this->assertSame('/api/applications/app_1', $this->calls[0]['endpoint']);
        $this->assertSame('/api/service-accounts/sa_1', $this->calls[1]['endpoint']);
        $this->assertSame('sa_1', $sa->id);
        $this->assertSame('orders-sa', $sa->code);
        $this->assertSame('prn_1', $sa->principalId);
        $this->assertSame('oac_1', $sa->oauthClientId);
    }

    public function test_get_service_account_is_not_found_when_the_application_has_none(): void
    {
        $apps = new Applications($this->client([
            ['id' => 'app_1', 'code' => 'orders', 'name' => 'Orders', 'type' => 'APPLICATION', 'active' => true,
                'hasLoginClient' => false, 'createdAt' => 't', 'updatedAt' => 't'],
        ]));

        try {
            $apps->getServiceAccount('app_1');
            $this->fail('expected a not-found error');
        } catch (FlowCatalystException $e) {
            $this->assertSame(404, $e->getCode());
        }
        $this->assertCount(1, $this->calls);
    }

    public function test_provision_service_account_keeps_the_nested_one_time_secret(): void
    {
        $apps = new Applications($this->client([[
            'message' => 'Service account provisioned',
            'serviceAccount' => [
                'principalId' => 'prn_1',
                'name' => 'Orders SA',
                'oauthClient' => ['id' => 'oac_1', 'clientId' => 'orders-sa', 'clientSecret' => 's3cret'],
            ],
        ]]));

        $result = $apps->provisionServiceAccount('app_1');

        $this->assertSame('/api/applications/app_1/provision-service-account', $this->calls[0]['endpoint']);
        $this->assertSame('Service account provisioned', $result->message);
        $this->assertSame('prn_1', $result->serviceAccount->principalId);
        $this->assertSame('orders-sa', $result->serviceAccount->oauthClient->clientId);
        $this->assertSame('s3cret', $result->serviceAccount->oauthClient->clientSecret);
    }

    public function test_application_roles_are_names(): void
    {
        $apps = new Applications($this->client([
            ['roles' => ['orders:admin', 'orders:viewer']],
            ['roles' => ['orders:admin', 'orders:viewer']],
        ]));

        $this->assertSame(['orders:admin', 'orders:viewer'], $apps->listRoleNames('app_1'));
        $roles = $apps->listRoles('app_1');

        $this->assertSame('/api/applications/by-id/app_1/roles', $this->calls[0]['endpoint']);
        $this->assertCount(2, $roles);
        $this->assertSame('orders:admin', $roles[0]->code);
    }

    public function test_application_client_configs_read_items_and_config_json(): void
    {
        $row = ['id' => 'cfg_1', 'applicationId' => 'app_1', 'clientId' => 'clt_1', 'enabled' => true,
            'baseUrlOverride' => 'https://acme.test', 'configJson' => ['theme' => 'dark'],
            'createdAt' => 't1', 'updatedAt' => 't2'];
        $apps = new Applications($this->client([['items' => [$row]], $row]));

        $list = $apps->listClients('app_1');
        $one = $apps->getClientConfig('app_1', 'clt_1');

        $this->assertCount(1, $list->clientConfigs);
        $this->assertSame(['theme' => 'dark'], $list->clientConfigs[0]->config);
        $this->assertSame('https://acme.test', $list->clientConfigs[0]->baseUrlOverride);
        $this->assertSame('GET', $this->calls[1]['method']);
        $this->assertSame('/api/applications/app_1/clients/clt_1', $this->calls[1]['endpoint']);
        $this->assertTrue($one->enabled);
        $this->assertSame('t2', $one->updatedAt);
    }

    public function test_enable_and_disable_for_client_read_no_body(): void
    {
        $apps = new Applications($this->client([[], []]));

        $apps->enableForClient('app_1', 'clt_1');
        $apps->disableForClient('app_1', 'clt_1');

        $this->assertSame('/api/applications/app_1/clients/clt_1/enable', $this->calls[0]['endpoint']);
        $this->assertSame('/api/applications/app_1/clients/clt_1/disable', $this->calls[1]['endpoint']);
    }

    // ── audit logs ──────────────────────────────────────────────────────

    public function test_audit_logs_page_by_cursor_and_filter_by_client_ids(): void
    {
        $logs = new AuditLogs($this->client([[
            'auditLogs' => [['id' => 'aud_1', 'operation' => 'CreateOrder', 'entityType' => 'Order',
                'entityId' => 'ord_1', 'performedAt' => 't', 'operationJson' => '{}']],
            'hasMore' => true,
            'nextCursor' => 'cur_2',
        ]]));

        $page = $logs->list(entityType: 'Order', clientId: 'clt_1', pageSize: 50, after: 'cur_1',
            applicationIds: ['app_1', 'app_2']);

        $this->assertSame([
            'after' => 'cur_1',
            'pageSize' => '50',
            'entityType' => 'Order',
            'applicationIds' => 'app_1,app_2',
            'clientIds' => 'clt_1',
        ], $this->query(0));
        $this->assertTrue($page->hasMore);
        $this->assertSame('cur_2', $page->nextCursor);
        $this->assertSame('{}', $page->auditLogs[0]->operationJson);
    }

    // ── clients ─────────────────────────────────────────────────────────

    public function test_client_list_applies_the_status_filter_itself(): void
    {
        $clients = new Clients($this->client([['clients' => [
            ['id' => 'c1', 'name' => 'A', 'identifier' => 'a', 'status' => 'ACTIVE', 'notes' => []],
            ['id' => 'c2', 'name' => 'B', 'identifier' => 'b', 'status' => 'SUSPENDED', 'notes' => []],
        ], 'total' => 2]]));

        $list = $clients->list('ACTIVE');

        $this->assertSame(['c1'], array_map(static fn($c) => $c->id, $list->clients));
        $this->assertSame(1, $list->total);
    }

    public function test_client_update_reads_no_body(): void
    {
        $clients = new Clients($this->client([[]]));

        $clients->update('c1', new UpdateClientRequest(name: 'A2'));

        $this->assertSame('PUT', $this->calls[0]['method']);
    }

    // ── connections ─────────────────────────────────────────────────────

    public function test_connection_update_always_sends_name(): void
    {
        $connection = ['id' => 'con_1', 'code' => 'hook', 'name' => 'Hook', 'status' => 'ACTIVE',
            'serviceAccountId' => 'sa_1', 'source' => 'API', 'createdAt' => 't', 'updatedAt' => 't'];
        $connections = new Connections($this->client([$connection, [], []]));

        $connections->update('con_1', new UpdateConnectionRequest(description: 'new'));
        $connections->update('con_1', new UpdateConnectionRequest(name: 'Renamed', applicationCode: 'orders'));

        $this->assertSame(['GET', 'PUT', 'PUT'], array_column($this->calls, 'method'));
        $this->assertSame(['name' => 'Hook', 'description' => 'new'], $this->calls[1]['options']['json']);
        $this->assertSame(['name' => 'Renamed', 'applicationCode' => 'orders'], $this->calls[2]['options']['json']);
    }

    public function test_connection_list_applies_the_service_account_filter_itself(): void
    {
        $connections = new Connections($this->client([['connections' => [
            ['id' => 'con_1', 'code' => 'a', 'name' => 'A', 'status' => 'ACTIVE', 'serviceAccountId' => 'sa_1'],
            ['id' => 'con_2', 'code' => 'b', 'name' => 'B', 'status' => 'ACTIVE', 'serviceAccountId' => 'sa_2'],
        ], 'total' => 2]]));

        $list = $connections->list(clientId: 'clt_1', serviceAccountId: 'sa_2');

        $this->assertSame('clt_1', $this->query(0)['clientId']);
        $this->assertSame(['con_2'], array_map(static fn($c) => $c->id, $list->connections));
        $this->assertSame(1, $list->total);
    }

    // ── dispatch pools ──────────────────────────────────────────────────

    public function test_dispatch_pools_list_reads_pools_and_transitions_read_no_body(): void
    {
        $pools = new DispatchPools($this->client([
            ['pools' => [['id' => 'dp_1', 'code' => 'default', 'name' => 'Default', 'concurrency' => 5,
                'status' => 'ACTIVE', 'createdAt' => 't', 'updatedAt' => 't']], 'total' => 1],
            [], [], [],
        ]));

        $list = $pools->list();
        $pools->archive('dp_1');
        $pools->suspend('dp_1');
        $pools->activate('dp_1');

        $this->assertCount(1, $list);
        $this->assertSame('default', $list[0]->code);
        $this->assertSame(['GET', 'POST', 'POST', 'POST'], array_column($this->calls, 'method'));
        $this->assertSame('/api/dispatch-pools/dp_1/archive', $this->calls[1]['endpoint']);
        $this->assertSame('/api/dispatch-pools/dp_1/suspend', $this->calls[2]['endpoint']);
        $this->assertSame('/api/dispatch-pools/dp_1/activate', $this->calls[3]['endpoint']);
    }

    // ── event types ─────────────────────────────────────────────────────

    public function test_event_type_update_always_sends_name(): void
    {
        $eventType = ['id' => 'evt_1', 'code' => 'orders:sales:order:created', 'name' => 'Order Created',
            'eventName' => 'created', 'application' => 'orders', 'subdomain' => 'sales', 'aggregate' => 'order',
            'source' => 'API', 'status' => 'CURRENT', 'specVersions' => [], 'createdAt' => 't', 'updatedAt' => 't'];
        $eventTypes = new EventTypes($this->client([$eventType, [], []]));

        $eventTypes->update('evt_1', new UpdateEventTypeRequest(description: 'd'));
        $eventTypes->update('evt_1', new UpdateEventTypeRequest(name: 'N', clientScoped: true));

        $this->assertSame(['GET', 'PUT', 'PUT'], array_column($this->calls, 'method'));
        $this->assertSame(['name' => 'Order Created', 'description' => 'd'], $this->calls[1]['options']['json']);
        $this->assertSame(['name' => 'N', 'clientScoped' => true], $this->calls[2]['options']['json']);
    }

    public function test_event_type_create_sends_client_scoped_only_when_set(): void
    {
        $eventTypes = new EventTypes($this->client([['id' => 'evt_1'], ['id' => 'evt_2']]));

        $this->assertSame('evt_1', $eventTypes->create(
            new CreateEventTypeRequest(code: 'orders:sales:order:created', name: 'Created', clientScoped: true),
        ));
        $eventTypes->create(new CreateEventTypeRequest(code: 'orders:sales:order:paid', name: 'Paid'));

        $this->assertSame(
            ['code' => 'orders:sales:order:created', 'name' => 'Created', 'clientScoped' => true],
            $this->calls[0]['options']['json'],
        );
        $this->assertSame(['code' => 'orders:sales:order:paid', 'name' => 'Paid'], $this->calls[1]['options']['json']);
    }

    public function test_event_type_reads_event_name_and_archive_is_the_delete_route(): void
    {
        $eventTypes = new EventTypes($this->client([
            ['id' => 'evt_1', 'code' => 'orders:sales:order:created', 'name' => 'Order Created',
                'eventName' => 'created', 'application' => 'orders', 'subdomain' => 'sales', 'aggregate' => 'order',
                'source' => 'API', 'clientId' => 'clt_1', 'status' => 'CURRENT', 'specVersions' => [],
                'createdAt' => 't', 'updatedAt' => 't'],
            [], [],
        ]));

        $eventType = $eventTypes->get('evt_1');
        $eventTypes->delete('evt_1');
        $eventTypes->archive('evt_1');

        $this->assertSame('created', $eventType->event);
        $this->assertSame('API', $eventType->source);
        $this->assertSame('clt_1', $eventType->clientId);
        $this->assertSame(['GET', 'DELETE', 'DELETE'], array_column($this->calls, 'method'));
        $this->assertSame('/api/event-types/evt_1', $this->calls[2]['endpoint']);
    }

    public function test_event_type_list_sends_subdomain_and_aggregate(): void
    {
        $eventTypes = new EventTypes($this->client([['items' => []]]));

        $eventTypes->list(application: 'orders', subdomain: 'sales', aggregate: 'order');

        $this->assertSame(['application' => 'orders', 'subdomain' => 'sales', 'aggregate' => 'order'], $this->query(0));
    }

    // ── permissions ─────────────────────────────────────────────────────

    public function test_permission_reads_name_and_category_and_parses_the_segments(): void
    {
        $permissions = new Permissions($this->client([['permissions' => [[
            'permission' => 'orders:sales:order:read',
            'name' => 'Read orders',
            'description' => 'd',
            'category' => 'Orders',
        ]], 'total' => 1]]));

        $permission = $permissions->list()->permissions[0];

        $this->assertSame('Read orders', $permission->name);
        $this->assertSame('Orders', $permission->category);
        $this->assertSame('orders', $permission->application);
        $this->assertSame('sales', $permission->context);
        $this->assertSame('order', $permission->aggregate);
        $this->assertSame('read', $permission->action);
    }

    // ── principals ──────────────────────────────────────────────────────

    public function test_find_by_email_searches_with_q_and_matches_exactly(): void
    {
        $principals = new Principals($this->client([['principals' => [
            ['id' => 'p1', 'type' => 'USER', 'name' => 'A', 'email' => 'ann@acme.test.au'],
            ['id' => 'p2', 'type' => 'USER', 'name' => 'B', 'email' => 'Ann@Acme.test'],
        ], 'total' => 2]]));

        $principal = $principals->findByEmail('ann@acme.test');

        $this->assertSame('ann@acme.test', $this->query(0)['q']);
        $this->assertSame('p2', $principal?->id);
    }

    public function test_principal_list_sends_q(): void
    {
        $principals = new Principals($this->client([['principals' => [], 'total' => 0]]));

        $principals->list(active: true, q: 'ann');

        $this->assertSame(['active' => 'true', 'q' => 'ann'], $this->query(0));
    }

    // ── roles ───────────────────────────────────────────────────────────

    public function test_role_list_applies_its_filters_itself_and_derives_short_name(): void
    {
        $roles = new Roles($this->client([['roles' => [
            ['id' => 'r1', 'name' => 'orders:admin', 'displayName' => 'Admin', 'applicationCode' => 'orders',
                'source' => 'SDK', 'permissions' => [], 'clientManaged' => false],
            ['id' => 'r2', 'name' => 'billing:admin', 'displayName' => 'Admin', 'applicationCode' => 'billing',
                'source' => 'SDK', 'permissions' => [], 'clientManaged' => false],
        ], 'total' => 2]]));

        $list = $roles->list(applicationCode: 'orders');

        $this->assertSame(['r1'], array_map(static fn($r) => $r->id, $list->roles));
        $this->assertSame(1, $list->total);
        $this->assertSame('admin', $list->roles[0]->shortName);
    }

    // ── router ──────────────────────────────────────────────────────────

    public function test_in_pipeline_exposes_the_top_level_pool_and_queue_as_detail(): void
    {
        $routerHttp = new Client([
            'handler' => HandlerStack::create(new MockHandler([
                new Response(200, [], '{"messageId":"m1","inPipeline":true,"poolCode":"default","queueId":"q1"}'),
                new Response(200, [], '{"messageId":"m2","inPipeline":false}'),
            ])),
            'http_errors' => false,
        ]);
        $client = new FlowCatalystClient(tokenProvider: new UserTokenProvider('tok'), baseUrl: 'https://fc.test');
        $router = new Router($client, $routerHttp);

        $hit = $router->inPipeline('m1');
        $miss = $router->inPipeline('m2');

        $this->assertTrue($hit['inPipeline']);
        $this->assertSame('default', $hit['poolCode']);
        $this->assertSame(['messageId' => 'm1', 'poolCode' => 'default', 'queueId' => 'q1'], $hit['detail']);
        $this->assertArrayNotHasKey('detail', $miss);
    }

    // ── scheduled jobs ──────────────────────────────────────────────────

    public function test_scheduled_job_lists_read_total_pages(): void
    {
        $jobs = new ScheduledJobs($this->client([
            ['data' => [], 'page' => 1, 'size' => 20, 'total' => 41, 'total_pages' => 3],
            ['data' => [], 'page' => 0, 'size' => 20, 'total' => 5, 'total_pages' => 1],
        ]));

        $this->assertSame(3, $jobs->list(page: 1)['totalPages']);
        $this->assertSame(1, $jobs->listInstances('sj_1')['totalPages']);
    }

    // ── subscriptions ───────────────────────────────────────────────────

    public function test_subscription_pause_and_resume_read_no_body(): void
    {
        $subscriptions = new Subscriptions($this->client([[], []]));

        $subscriptions->pause('sub_1');
        $subscriptions->resume('sub_1');

        $this->assertSame('/api/subscriptions/sub_1/pause', $this->calls[0]['endpoint']);
        $this->assertSame('/api/subscriptions/sub_1/resume', $this->calls[1]['endpoint']);
    }
}
