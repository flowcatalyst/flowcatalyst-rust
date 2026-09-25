<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Attributes;

use FlowCatalyst\Attributes\AsPermission;
use FlowCatalyst\Attributes\AsRole;
use FlowCatalyst\DTOs\PermissionInput;
use PHPUnit\Framework\TestCase;

#[AsPermission(context: 'posts', aggregate: 'post', action: 'view')]
class FixtureViewPosts {}

#[AsPermission(context: 'posts', aggregate: 'post', action: 'edit', application: 'override')]
class FixtureEditPosts {}

final class PermissionDefinitionTest extends TestCase
{
    public function test_as_permission_uses_default_application(): void
    {
        $permission = new AsPermission(context: 'orders', aggregate: 'order', action: 'view');

        $this->assertSame('app:orders:order:view', $permission->toPermissionString('app'));
        $this->assertSame(['permission' => 'app:orders:order:view'], $permission->toArray('app'));
        $this->assertInstanceOf(PermissionInput::class, $permission->toPermissionInput('app'));
    }

    public function test_as_permission_explicit_application_and_description_win(): void
    {
        $permission = new AsPermission(
            context: 'orders',
            aggregate: 'order',
            action: 'view',
            application: 'fixed',
            description: 'See orders',
        );

        $this->assertSame('fixed:orders:order:view', $permission->toPermissionString('ignored'));
        $this->assertSame(
            ['permission' => 'fixed:orders:order:view', 'description' => 'See orders'],
            $permission->toArray('ignored'),
        );
    }

    public function test_as_permission_requires_an_application(): void
    {
        $this->expectException(\InvalidArgumentException::class);
        (new AsPermission('a', 'b', 'c'))->toPermissionString(null);
    }

    public function test_role_resolves_mixed_permission_forms(): void
    {
        $role = new AsRole(
            name: 'editor',
            permissions: [
                new PermissionInput('myapp', 'posts', 'post', 'publish'), // structured
                FixtureViewPosts::class,                                  // #[AsPermission] class-string
                FixtureEditPosts::class,                                  // class-string w/ explicit app
                'literal:perm:code:HERE',                                 // literal string (lower-cased)
            ],
        );

        $expected = [
            'myapp:posts:post:publish',
            'myapp:posts:post:view',
            'override:posts:post:edit',
            'literal:perm:code:here',
        ];

        $this->assertSame($expected, $role->permissionStrings('myapp'));
        $this->assertSame($expected, $role->toArray('myapp')['permissions']);
    }

    public function test_role_rejects_permission_class_without_attribute(): void
    {
        $role = new AsRole(name: 'x', permissions: [\stdClass::class]);

        $this->expectException(\InvalidArgumentException::class);
        $role->permissionStrings('myapp');
    }
}
