<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Sync;

use FlowCatalyst\Attributes\AsRole;
use FlowCatalyst\DTOs\PermissionInput;
use FlowCatalyst\Sync\RoleDefinition;
use PHPUnit\Framework\TestCase;

/**
 * Locks the roles-sync wire contract: permissions must serialize as a flat
 * list of "app:context:aggregate:action" strings (what the API's
 * SyncRoleInputRequest / Go syncRoleInputRequest expect), NOT as
 * {application,context,aggregate,action} objects.
 */
final class RolePermissionSerializationTest extends TestCase
{
    public function test_role_definition_serializes_permissions_as_strings(): void
    {
        $role = RoleDefinition::make('admin')
            ->withDisplayName('Administrator')
            ->withPermissions([
                PermissionInput::make('myapp', 'users', 'user', 'view'),
                PermissionInput::make('myapp', 'users', 'user', 'create'),
            ]);

        $this->assertSame(
            ['myapp:users:user:view', 'myapp:users:user:create'],
            $role->toArray()['permissions'],
        );
    }

    public function test_as_role_attribute_serializes_permissions_as_strings(): void
    {
        $role = new AsRole(
            name: 'editor',
            permissions: [PermissionInput::make('myapp', 'content', 'post', 'edit')],
        );

        $this->assertSame(['myapp:content:post:edit'], $role->toArray()['permissions']);
    }

    public function test_from_array_round_trips_the_string_shape(): void
    {
        $original = RoleDefinition::make('admin')->withPermissions([
            PermissionInput::make('myapp', 'orders', 'order', 'view'),
        ]);

        $restored = RoleDefinition::fromArray($original->toArray());

        $this->assertSame($original->toArray(), $restored->toArray());
        $this->assertSame('myapp:orders:order:view', $restored->permissions[0]->toPermissionString());
    }

    public function test_from_array_still_accepts_the_legacy_object_shape(): void
    {
        $restored = RoleDefinition::fromArray([
            'name' => 'admin',
            'permissions' => [
                ['application' => 'myapp', 'context' => 'orders', 'aggregate' => 'order', 'action' => 'view'],
            ],
        ]);

        $this->assertSame(['myapp:orders:order:view'], $restored->toArray()['permissions']);
    }
}
