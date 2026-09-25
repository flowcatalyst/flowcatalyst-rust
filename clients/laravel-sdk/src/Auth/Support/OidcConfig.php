<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth\Support;

use FlowCatalyst\Exceptions\AuthenticationException;

/**
 * Reads the base OIDC config (base_url/client_id/client_secret/scope) shared
 * by the login/callback/refresh flows — anything that talks to
 * `/oauth/authorize` or `/oauth/token`.
 */
final class OidcConfig
{
    /**
     * @return array{base_url: string, client_id: string, client_secret?: string, scope?: string}
     * @throws AuthenticationException
     */
    public static function resolve(): array
    {
        $config = [
            'base_url' => config('flowcatalyst.base_url'),
            'client_id' => config('flowcatalyst.oidc.client_id'),
            'client_secret' => config('flowcatalyst.oidc.client_secret'),
            'scope' => config('flowcatalyst.oidc.scope', 'openid profile email'),
        ];

        if (empty($config['base_url'])) {
            throw AuthenticationException::missingConfiguration('FLOWCATALYST_BASE_URL');
        }

        if (empty($config['client_id'])) {
            throw AuthenticationException::missingConfiguration('FLOWCATALYST_OIDC_CLIENT_ID');
        }

        return $config;
    }
}
