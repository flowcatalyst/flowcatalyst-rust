<?php

declare(strict_types=1);

namespace FlowCatalyst\Exceptions;

class OutboxException extends FlowCatalystException
{
    /**
     * Create an exception for missing client ID.
     */
    public static function missingClientId(): static
    {
        return new static('Client ID is required. Please set FLOWCATALYST_CLIENT_ID in your environment.');
    }

    /**
     * Create an exception for outbox not enabled.
     */
    public static function notEnabled(): static
    {
        return new static('Outbox is not enabled. Set FLOWCATALYST_OUTBOX_ENABLED=true in your environment.');
    }

    /**
     * Create an exception for driver not found.
     */
    public static function driverNotFound(string $driver): static
    {
        return new static("Outbox driver '{$driver}' is not supported. Supported drivers: database, mongodb.");
    }

    /**
     * Create an exception for insertion failure.
     */
    public static function insertFailed(string $reason): static
    {
        return new static("Failed to insert outbox message: {$reason}");
    }

    /**
     * Create an exception for MongoDB not installed.
     */
    public static function mongoDbNotInstalled(): static
    {
        return new static('MongoDB driver requires mongodb/laravel-mongodb package. Run: composer require mongodb/laravel-mongodb');
    }

    /**
     * Create an exception for an outbox write attempted outside of a database
     * transaction when `outbox.strict_transactions` is enabled.
     *
     * Strict mode catches the most common transactional-outbox bug: writing
     * the outbox row without wrapping it in `DB::transaction(...)` alongside
     * the business writes, so business state and outbox state can drift on
     * partial failure.
     */
    public static function noActiveTransaction(?string $connection = null): static
    {
        $conn = $connection ?? '(default)';
        return new static(
            "Outbox write attempted without an active database transaction on connection '{$conn}'. "
            . 'Wrap the call in DB::transaction(fn () => ...) so the outbox row commits atomically '
            . 'with your business writes. To downgrade this error to a warning, set '
            . "config('flowcatalyst.outbox.strict_transactions') to false."
        );
    }
}
