<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Auth;

use FlowCatalyst\Auth\Support\TokenScopePermissionResolver;
use PHPUnit\Framework\TestCase;

final class TokenScopePermissionResolverTest extends TestCase
{
    /** Build a JWT whose payload is the given claims (signature is irrelevant). */
    private function jwt(array $claims): string
    {
        $b64 = static fn (array $a): string => rtrim(strtr(base64_encode(json_encode($a)), '+/', '-_'), '=');
        return $b64(['alg' => 'RS256']) . '.' . $b64($claims) . '.sig';
    }

    public function test_reads_space_delimited_scope_claim(): void
    {
        $token = $this->jwt(['sub' => 'x', 'scope' => 'blog:posts:post:view blog:posts:post:edit']);

        $this->assertSame(
            ['blog:posts:post:view', 'blog:posts:post:edit'],
            (new TokenScopePermissionResolver())->resolve([], $token),
        );
    }

    public function test_reads_array_scope_claim(): void
    {
        $token = $this->jwt(['scope' => ['a:b:c:d', 'e:f:g:h']]);

        $this->assertSame(['a:b:c:d', 'e:f:g:h'], (new TokenScopePermissionResolver())->resolve([], $token));
    }

    public function test_falls_back_to_permissions_array_claim(): void
    {
        $token = $this->jwt(['permissions' => ['x:y:z:read']]);

        $this->assertSame(['x:y:z:read'], (new TokenScopePermissionResolver())->resolve([], $token));
    }

    public function test_empty_for_missing_or_malformed_token(): void
    {
        $resolver = new TokenScopePermissionResolver();
        $this->assertSame([], $resolver->resolve([], null));
        $this->assertSame([], $resolver->resolve([], ''));
        $this->assertSame([], $resolver->resolve([], 'not-a-jwt'));
        $this->assertSame([], $resolver->resolve([], $this->jwt(['sub' => 'x'])));
    }
}
