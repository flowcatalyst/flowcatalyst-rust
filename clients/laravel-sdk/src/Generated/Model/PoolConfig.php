<?php

namespace FlowCatalyst\Generated\Model;

class PoolConfig
{
    /**
     * @var array
     */
    protected $initialized = [];
    public function isInitialized($property): bool
    {
        return array_key_exists($property, $this->initialized);
    }
    /**
     * @var string|null
     */
    protected $code;
    /**
     * @var int|null
     */
    protected $concurrency;
    /**
     * @var int|null
     */
    protected $rateLimitPerMinute;
    /**
     * @return string|null
     */
    public function getCode(): ?string
    {
        return $this->code;
    }
    /**
     * @param string|null $code
     *
     * @return self
     */
    public function setCode(?string $code): self
    {
        $this->initialized['code'] = true;
        $this->code = $code;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getConcurrency(): ?int
    {
        return $this->concurrency;
    }
    /**
     * @param int|null $concurrency
     *
     * @return self
     */
    public function setConcurrency(?int $concurrency): self
    {
        $this->initialized['concurrency'] = true;
        $this->concurrency = $concurrency;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getRateLimitPerMinute(): ?int
    {
        return $this->rateLimitPerMinute;
    }
    /**
     * @param int|null $rateLimitPerMinute
     *
     * @return self
     */
    public function setRateLimitPerMinute(?int $rateLimitPerMinute): self
    {
        $this->initialized['rateLimitPerMinute'] = true;
        $this->rateLimitPerMinute = $rateLimitPerMinute;
        return $this;
    }
}