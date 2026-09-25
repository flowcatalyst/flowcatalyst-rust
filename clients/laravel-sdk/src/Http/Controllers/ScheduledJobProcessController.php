<?php

declare(strict_types=1);

namespace FlowCatalyst\Http\Controllers;

use FlowCatalyst\ScheduledJobs\ScheduledJobRunner;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;

/**
 * Invokable controller behind the auto-mounted scheduled-job process route.
 *
 * A controller class — NOT a closure — because `php artisan route:cache`
 * cannot serialize closure routes (a closure registered from the service
 * provider fatals route caching with infinite recursion on its captured
 * provider scope). The class reference serializes cleanly; the runner is
 * resolved from the container per request.
 */
final class ScheduledJobProcessController
{
    public function __invoke(Request $request, ScheduledJobRunner $runner): JsonResponse
    {
        [$status, $body] = $runner->processWithResponse((array) $request->json()->all());

        return response()->json($body, $status);
    }
}
