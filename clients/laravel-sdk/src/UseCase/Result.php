<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * Sealed Result<T> — either a success carrying a value or a failure carrying
 * a `UseCaseError`. Successes can only be built through a `UnitOfWork`.
 *
 * @template T
 */
final class Result
{
    private const TAG_SUCCESS = 'success';
    private const TAG_FAILURE = 'failure';

    /**
     * @param T|null $value
     */
    private function __construct(
        private readonly string $tag,
        private readonly mixed $value = null,
        private readonly ?UseCaseError $error = null,
    ) {}

    /**
     * Build a successful Result. Restricted — `UnitOfWork` implementations
     * hold the only instance of `ResultSuccessToken`, so use cases route
     * success through `unitOfWork->commit(...)`.
     *
     * @template U
     * @param U $value
     * @return self<U>
     *
     * @internal
     */
    public static function success(ResultSuccessToken $token, mixed $value): self
    {
        // Accepting the token is the seal — only callers who already hold it
        // (i.e. UnitOfWork implementations) can reach this path. We still
        // validate identity in case a caller tries to construct one via
        // reflection with a fake token shape.
        if ($token !== ResultSuccessToken::internal()) {
            throw new \LogicException(
                'Result::success() is restricted. Return success via UnitOfWork::commit() so domain events are always dispatched.',
            );
        }
        return new self(self::TAG_SUCCESS, value: $value);
    }

    /**
     * @template U
     * @return self<U>
     */
    public static function failure(UseCaseError $error): self
    {
        return new self(self::TAG_FAILURE, error: $error);
    }

    public function isSuccess(): bool
    {
        return $this->tag === self::TAG_SUCCESS;
    }

    public function isFailure(): bool
    {
        return $this->tag === self::TAG_FAILURE;
    }

    /**
     * @return T
     * @throws \LogicException when the result is a failure
     */
    public function value(): mixed
    {
        if (! $this->isSuccess()) {
            throw new \LogicException(
                'Cannot read value from failure Result. Use isSuccess() to guard or match()/unwrapOr() to handle.',
            );
        }
        return $this->value;
    }

    public function error(): UseCaseError
    {
        if (! $this->isFailure()) {
            throw new \LogicException(
                'Cannot read error from success Result. Use isFailure() to guard or match() to handle.',
            );
        }
        /** @var UseCaseError $err */
        $err = $this->error;
        return $err;
    }

    /**
     * @template U
     * @param callable(T): U $onSuccess
     * @param callable(UseCaseError): U $onFailure
     * @return U
     */
    public function match(callable $onSuccess, callable $onFailure): mixed
    {
        return $this->isSuccess() ? $onSuccess($this->value) : $onFailure($this->error());
    }

    /**
     * @template U
     * @param callable(T): U $fn
     * @return self<U>
     */
    public function map(callable $fn): self
    {
        if ($this->isSuccess()) {
            return new self(self::TAG_SUCCESS, value: $fn($this->value));
        }
        return new self(self::TAG_FAILURE, error: $this->error);
    }

    /**
     * @param T $default
     * @return T
     */
    public function unwrapOr(mixed $default): mixed
    {
        return $this->isSuccess() ? $this->value : $default;
    }
}
