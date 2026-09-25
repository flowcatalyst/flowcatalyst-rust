<?php

namespace FlowCatalyst\Generated\Model;

class QueueConfig
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
     * @var int|null
     */
    protected $connections;
    /**
     * @var string|null
     */
    protected $queueName;
    /**
     * @var string|null
     */
    protected $queueUri;
    /**
     * @var int|null
     */
    protected $visibilityTimeout;
    /**
     * @return int|null
     */
    public function getConnections(): ?int
    {
        return $this->connections;
    }
    /**
     * @param int|null $connections
     *
     * @return self
     */
    public function setConnections(?int $connections): self
    {
        $this->initialized['connections'] = true;
        $this->connections = $connections;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getQueueName(): ?string
    {
        return $this->queueName;
    }
    /**
     * @param string|null $queueName
     *
     * @return self
     */
    public function setQueueName(?string $queueName): self
    {
        $this->initialized['queueName'] = true;
        $this->queueName = $queueName;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getQueueUri(): ?string
    {
        return $this->queueUri;
    }
    /**
     * @param string|null $queueUri
     *
     * @return self
     */
    public function setQueueUri(?string $queueUri): self
    {
        $this->initialized['queueUri'] = true;
        $this->queueUri = $queueUri;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getVisibilityTimeout(): ?int
    {
        return $this->visibilityTimeout;
    }
    /**
     * @param int|null $visibilityTimeout
     *
     * @return self
     */
    public function setVisibilityTimeout(?int $visibilityTimeout): self
    {
        $this->initialized['visibilityTimeout'] = true;
        $this->visibilityTimeout = $visibilityTimeout;
        return $this;
    }
}