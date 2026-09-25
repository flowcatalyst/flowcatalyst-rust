<?php

declare(strict_types=1);

namespace FlowCatalyst\Http\Middleware;

use Closure;
use FlowCatalyst\Exceptions\WebhookValidationException;
use FlowCatalyst\Webhook\BearerTokenCheck;
use FlowCatalyst\Webhook\WebhookValidator;
use Illuminate\Http\Request;
use Symfony\Component\HttpFoundation\Response;

/**
 * Middleware to validate FlowCatalyst webhook signatures.
 *
 * Usage in routes:
 *   Route::post('/webhooks/flowcatalyst', [WebhookController::class, 'handle'])
 *       ->middleware('flowcatalyst.webhook');
 */
class ValidateWebhookSignature
{
    /**
     * Handle an incoming request.
     *
     * @param Closure(Request): Response $next
     */
    public function handle(Request $request, Closure $next): Response
    {
        try {
            $validator = WebhookValidator::fromConfig();
            $validator->validateRequest($request);
        } catch (WebhookValidationException $e) {
            return response()->json([
                'error' => $e->getMessage(),
            ], $e->getCode() ?: 401);
        }

        // Optional second gate, AND-ed with the signature (never a substitute
        // for it): when flowcatalyst.webhook_auth_token is configured, the
        // delivery must also carry the service account's bearer token.
        $bearerError = BearerTokenCheck::check(
            config('flowcatalyst.webhook_auth_token'),
            $request->header('Authorization'),
        );
        if ($bearerError !== null) {
            return response()->json(['error' => $bearerError], 401);
        }

        return $next($request);
    }
}
