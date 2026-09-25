<?php

declare(strict_types=1);

namespace FlowCatalyst\Client;

use FlowCatalyst\Exceptions\FlowCatalystException;

/**
 * @internal Control-flow-only signal used by {@see FlowCatalystClient::request()}
 * to mark a failure as eligible for backoff retry, without changing the
 * type, message, code, or `$data` of the exception that is ultimately
 * thrown to callers once retries are exhausted (or immediately, when the
 * failure is not retryable). Never escapes the client's public API.
 */
final class RetryableFailure extends \RuntimeException
{
    public function __construct(public readonly FlowCatalystException $exception)
    {
        parent::__construct($exception->getMessage(), 0, $exception);
    }
}
