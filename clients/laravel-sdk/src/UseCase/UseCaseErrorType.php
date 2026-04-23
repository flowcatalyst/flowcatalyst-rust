<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * Categories of use case failures. Maps to HTTP status codes for API layers.
 */
enum UseCaseErrorType: string
{
    case Validation      = 'validation';
    case NotFound        = 'not_found';
    case BusinessRule    = 'business_rule';
    case Concurrency     = 'concurrency';
    case Authorization   = 'authorization';
    case Infrastructure  = 'infrastructure';

    public function httpStatus(): int
    {
        return match ($this) {
            self::Validation                     => 400,
            self::Authorization                  => 403,
            self::NotFound                       => 404,
            self::BusinessRule, self::Concurrency => 409,
            self::Infrastructure                 => 500,
        };
    }
}
