<?php

declare(strict_types=1);

namespace FlowCatalyst\Attributes;

use Attribute;

/**
 * Marks a class as a dispatch pool definition for FlowCatalyst.
 *
 * Usage:
 * ```php
 * #[AsDispatchPool(
 *     code: 'default',
 *     name: 'Default Pool',
 *     description: 'Default dispatch pool for standard rate limiting',
 *     rateLimit: 100,
 *     concurrency: 10
 * )]
 * class DefaultDispatchPool {}
 * ```
 *
 * Dispatch pools control the rate at which dispatch jobs can be processed.
 * They define rate limits (per minute) and concurrency limits.
 */
#[Attribute(Attribute::TARGET_CLASS)]
final class AsDispatchPool
{
    /**
     * Pool code format: lowercase alphanumeric with hyphens/underscores, starting with letter.
     */
    private const CODE_PATTERN = '/^[a-z][a-z0-9_-]*$/';

    /**
     * @param string $code Unique pool code (e.g., 'default', 'high-priority')
     * @param string|null $name Human-readable name
     * @param string|null $description Pool description
     * @param int $rateLimit Maximum dispatches per minute (default 100)
     * @param int $concurrency Maximum concurrent dispatches (default 10)
     * @param string|null $application Application code this pool belongs to.
     *        Overrides the global `application_code` / `application_map` during
     *        sync — set it when one codebase defines dispatch pools for more
     *        than one application. Null = resolve from the namespace map /
     *        default.
     */
    public function __construct(
        public readonly string $code,
        public readonly ?string $name = null,
        public readonly ?string $description = null,
        public readonly int $rateLimit = 100,
        public readonly int $concurrency = 10,
        public readonly ?string $application = null,
    ) {}

    /**
     * Validate the pool code.
     *
     * @return string|null Error message if invalid, null if valid
     */
    public function validate(): ?string
    {
        if (empty($this->code)) {
            return 'Pool code cannot be empty';
        }

        if (!preg_match(self::CODE_PATTERN, $this->code)) {
            return "Pool code must be lowercase alphanumeric with hyphens/underscores, starting with a letter: {$this->code}";
        }

        return null;
    }

    /**
     * Check if this pool definition is valid.
     */
    public function isValid(): bool
    {
        return $this->validate() === null;
    }

    /**
     * Convert to array format for API sync.
     */
    public function toArray(): array
    {
        $data = [
            'code' => $this->code,
            'rateLimit' => $this->rateLimit,
            'concurrency' => $this->concurrency,
        ];

        if ($this->name !== null) {
            $data['name'] = $this->name;
        }

        if ($this->description !== null) {
            $data['description'] = $this->description;
        }

        return $data;
    }
}
