<?php

namespace FlowCatalyst\Generated\Model;

class CreateSubscriptionRequest extends \ArrayObject
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
     * @var string|null
     */
    protected $clientId;
    /**
     * @var string|null
     */
    protected $code;
    /**
     * @var string|null
     */
    protected $connectionId;
    /**
     * @var list<ConfigEntryDTO>|null
     */
    protected $customConfig;
    /**
     * @var bool|null
     */
    protected $dataOnly;
    /**
     * @var int|null
     */
    protected $delaySeconds;
    /**
     * @var string|null
     */
    protected $description;
    /**
     * @var string|null
     */
    protected $dispatchPoolId;
    /**
     * http(s) URL delivery target
     *
     * @var string|null
     */
    protected $endpoint;
    /**
     * @var list<EventTypeBindingDTO>|null
     */
    protected $eventTypes;
    /**
     * @var int|null
     */
    protected $maxAgeSeconds;
    /**
     * @var int|null
     */
    protected $maxRetries;
    /**
     * Dispatch mode. Omitted means NEXT_ON_ERROR: a message group is delivered in sequence, one at a time, moving on past a failure. BLOCK_ON_ERROR stops the group at a failure instead; IMMEDIATE opts out of ordering entirely. Ordering applies only to messages that carry a message group.
     *
     * @var string|null
     */
    protected $mode;
    /**
     * @var string|null
     */
    protected $name;
    /**
     * Dispatch priority: DEFAULT or HIGH_PRIORITY (matched ignoring case). Selects which of the client's two dispatch queues jobs raised from this subscription publish to. Omitted or blank leaves it unset, which publishes as DEFAULT.
     *
     * @var string|null
     */
    protected $queue;
    /**
     * @var string|null
     */
    protected $serviceAccountId;
    /**
     * @var int|null
     */
    protected $timeoutSeconds;
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
     * @return string|null
     */
    public function getClientId(): ?string
    {
        return $this->clientId;
    }
    /**
     * @param string|null $clientId
     *
     * @return self
     */
    public function setClientId(?string $clientId): self
    {
        $this->initialized['clientId'] = true;
        $this->clientId = $clientId;
        return $this;
    }
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
     * @return string|null
     */
    public function getConnectionId(): ?string
    {
        return $this->connectionId;
    }
    /**
     * @param string|null $connectionId
     *
     * @return self
     */
    public function setConnectionId(?string $connectionId): self
    {
        $this->initialized['connectionId'] = true;
        $this->connectionId = $connectionId;
        return $this;
    }
    /**
     * @return list<ConfigEntryDTO>|null
     */
    public function getCustomConfig(): ?array
    {
        return $this->customConfig;
    }
    /**
     * @param list<ConfigEntryDTO>|null $customConfig
     *
     * @return self
     */
    public function setCustomConfig(?array $customConfig): self
    {
        $this->initialized['customConfig'] = true;
        $this->customConfig = $customConfig;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getDataOnly(): ?bool
    {
        return $this->dataOnly;
    }
    /**
     * @param bool|null $dataOnly
     *
     * @return self
     */
    public function setDataOnly(?bool $dataOnly): self
    {
        $this->initialized['dataOnly'] = true;
        $this->dataOnly = $dataOnly;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getDelaySeconds(): ?int
    {
        return $this->delaySeconds;
    }
    /**
     * @param int|null $delaySeconds
     *
     * @return self
     */
    public function setDelaySeconds(?int $delaySeconds): self
    {
        $this->initialized['delaySeconds'] = true;
        $this->delaySeconds = $delaySeconds;
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
    public function getDispatchPoolId(): ?string
    {
        return $this->dispatchPoolId;
    }
    /**
     * @param string|null $dispatchPoolId
     *
     * @return self
     */
    public function setDispatchPoolId(?string $dispatchPoolId): self
    {
        $this->initialized['dispatchPoolId'] = true;
        $this->dispatchPoolId = $dispatchPoolId;
        return $this;
    }
    /**
     * http(s) URL delivery target
     *
     * @return string|null
     */
    public function getEndpoint(): ?string
    {
        return $this->endpoint;
    }
    /**
     * http(s) URL delivery target
     *
     * @param string|null $endpoint
     *
     * @return self
     */
    public function setEndpoint(?string $endpoint): self
    {
        $this->initialized['endpoint'] = true;
        $this->endpoint = $endpoint;
        return $this;
    }
    /**
     * @return list<EventTypeBindingDTO>|null
     */
    public function getEventTypes(): ?array
    {
        return $this->eventTypes;
    }
    /**
     * @param list<EventTypeBindingDTO>|null $eventTypes
     *
     * @return self
     */
    public function setEventTypes(?array $eventTypes): self
    {
        $this->initialized['eventTypes'] = true;
        $this->eventTypes = $eventTypes;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getMaxAgeSeconds(): ?int
    {
        return $this->maxAgeSeconds;
    }
    /**
     * @param int|null $maxAgeSeconds
     *
     * @return self
     */
    public function setMaxAgeSeconds(?int $maxAgeSeconds): self
    {
        $this->initialized['maxAgeSeconds'] = true;
        $this->maxAgeSeconds = $maxAgeSeconds;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getMaxRetries(): ?int
    {
        return $this->maxRetries;
    }
    /**
     * @param int|null $maxRetries
     *
     * @return self
     */
    public function setMaxRetries(?int $maxRetries): self
    {
        $this->initialized['maxRetries'] = true;
        $this->maxRetries = $maxRetries;
        return $this;
    }
    /**
     * Dispatch mode. Omitted means NEXT_ON_ERROR: a message group is delivered in sequence, one at a time, moving on past a failure. BLOCK_ON_ERROR stops the group at a failure instead; IMMEDIATE opts out of ordering entirely. Ordering applies only to messages that carry a message group.
     *
     * @return string|null
     */
    public function getMode(): ?string
    {
        return $this->mode;
    }
    /**
     * Dispatch mode. Omitted means NEXT_ON_ERROR: a message group is delivered in sequence, one at a time, moving on past a failure. BLOCK_ON_ERROR stops the group at a failure instead; IMMEDIATE opts out of ordering entirely. Ordering applies only to messages that carry a message group.
     *
     * @param string|null $mode
     *
     * @return self
     */
    public function setMode(?string $mode): self
    {
        $this->initialized['mode'] = true;
        $this->mode = $mode;
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
     * Dispatch priority: DEFAULT or HIGH_PRIORITY (matched ignoring case). Selects which of the client's two dispatch queues jobs raised from this subscription publish to. Omitted or blank leaves it unset, which publishes as DEFAULT.
     *
     * @return string|null
     */
    public function getQueue(): ?string
    {
        return $this->queue;
    }
    /**
     * Dispatch priority: DEFAULT or HIGH_PRIORITY (matched ignoring case). Selects which of the client's two dispatch queues jobs raised from this subscription publish to. Omitted or blank leaves it unset, which publishes as DEFAULT.
     *
     * @param string|null $queue
     *
     * @return self
     */
    public function setQueue(?string $queue): self
    {
        $this->initialized['queue'] = true;
        $this->queue = $queue;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getServiceAccountId(): ?string
    {
        return $this->serviceAccountId;
    }
    /**
     * @param string|null $serviceAccountId
     *
     * @return self
     */
    public function setServiceAccountId(?string $serviceAccountId): self
    {
        $this->initialized['serviceAccountId'] = true;
        $this->serviceAccountId = $serviceAccountId;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getTimeoutSeconds(): ?int
    {
        return $this->timeoutSeconds;
    }
    /**
     * @param int|null $timeoutSeconds
     *
     * @return self
     */
    public function setTimeoutSeconds(?int $timeoutSeconds): self
    {
        $this->initialized['timeoutSeconds'] = true;
        $this->timeoutSeconds = $timeoutSeconds;
        return $this;
    }
}