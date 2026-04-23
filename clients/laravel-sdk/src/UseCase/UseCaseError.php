<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * Structured use case failure. Carries a machine-readable `code`, a
 * human-readable `message`, arbitrary `details`, and a `type` category
 * that drives HTTP status mapping.
 */
final class UseCaseError
{
    /**
     * @param array<string, mixed> $details
     */
    private function __construct(
        public readonly UseCaseErrorType $type,
        public readonly string $code,
        public readonly string $message,
        public readonly array $details = [],
    ) {}

    /** @param array<string, mixed> $details */
    public static function validation(string $code, string $message, array $details = []): self
    {
        return new self(UseCaseErrorType::Validation, $code, $message, $details);
    }

    /** @param array<string, mixed> $details */
    public static function notFound(string $code, string $message, array $details = []): self
    {
        return new self(UseCaseErrorType::NotFound, $code, $message, $details);
    }

    /** @param array<string, mixed> $details */
    public static function businessRule(string $code, string $message, array $details = []): self
    {
        return new self(UseCaseErrorType::BusinessRule, $code, $message, $details);
    }

    /** @param array<string, mixed> $details */
    public static function concurrency(string $code, string $message, array $details = []): self
    {
        return new self(UseCaseErrorType::Concurrency, $code, $message, $details);
    }

    /** @param array<string, mixed> $details */
    public static function authorization(string $code, string $message, array $details = []): self
    {
        return new self(UseCaseErrorType::Authorization, $code, $message, $details);
    }

    /** @param array<string, mixed> $details */
    public static function infrastructure(string $code, string $message, array $details = []): self
    {
        return new self(UseCaseErrorType::Infrastructure, $code, $message, $details);
    }

    public function httpStatus(): int
    {
        return $this->type->httpStatus();
    }

    /**
     * @return array{type: string, code: string, message: string, details: array<string, mixed>}
     */
    public function toArray(): array
    {
        return [
            'type'    => $this->type->value,
            'code'    => $this->code,
            'message' => $this->message,
            'details' => $this->details,
        ];
    }
}
