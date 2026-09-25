<?php

namespace FlowCatalyst\Generated\Model;

class RouterConfig
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
     * A URL to the JSON Schema for this object.
     *
     * @var string|null
     */
    protected $dollarSchema;
    /**
     * @var list<PoolConfig>|null
     */
    protected $processingPools;
    /**
     * @var list<QueueConfig>|null
     */
    protected $queues;
    /**
     * A URL to the JSON Schema for this object.
     *
     * @return string|null
     */
    public function getDollarSchema(): ?string
    {
        return $this->dollarSchema;
    }
    /**
     * A URL to the JSON Schema for this object.
     *
     * @param string|null $dollarSchema
     *
     * @return self
     */
    public function setDollarSchema(?string $dollarSchema): self
    {
        $this->initialized['dollarSchema'] = true;
        $this->dollarSchema = $dollarSchema;
        return $this;
    }
    /**
     * @return list<PoolConfig>|null
     */
    public function getProcessingPools(): ?array
    {
        return $this->processingPools;
    }
    /**
     * @param list<PoolConfig>|null $processingPools
     *
     * @return self
     */
    public function setProcessingPools(?array $processingPools): self
    {
        $this->initialized['processingPools'] = true;
        $this->processingPools = $processingPools;
        return $this;
    }
    /**
     * @return list<QueueConfig>|null
     */
    public function getQueues(): ?array
    {
        return $this->queues;
    }
    /**
     * @param list<QueueConfig>|null $queues
     *
     * @return self
     */
    public function setQueues(?array $queues): self
    {
        $this->initialized['queues'] = true;
        $this->queues = $queues;
        return $this;
    }
}