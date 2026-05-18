<?php

namespace FlowCatalyst\Generated\Model;

class SyncDispatchPoolInputRequest extends \ArrayObject
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
     * @var string|null
     */
    protected $description;
    /**
     * @var string|null
     */
    protected $name;
    /**
     * Optional. `None` / omitted = concurrency-only (no rate limit).
     *
     * @var int|null
     */
    protected $rateLimit;
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
     * @return string|null
     */
    public function getDescription(): ?string
    {
        return $this->description;
    }
    /**
     * @param string|null $description
     *
     * @return self
     */
    public function setDescription(?string $description): self
    {
        $this->initialized['description'] = true;
        $this->description = $description;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getName(): ?string
    {
        return $this->name;
    }
    /**
     * @param string|null $name
     *
     * @return self
     */
    public function setName(?string $name): self
    {
        $this->initialized['name'] = true;
        $this->name = $name;
        return $this;
    }
    /**
     * Optional. `None` / omitted = concurrency-only (no rate limit).
     *
     * @return int|null
     */
    public function getRateLimit(): ?int
    {
        return $this->rateLimit;
    }
    /**
     * Optional. `None` / omitted = concurrency-only (no rate limit).
     *
     * @param int|null $rateLimit
     *
     * @return self
     */
    public function setRateLimit(?int $rateLimit): self
    {
        $this->initialized['rateLimit'] = true;
        $this->rateLimit = $rateLimit;
        return $this;
    }
}