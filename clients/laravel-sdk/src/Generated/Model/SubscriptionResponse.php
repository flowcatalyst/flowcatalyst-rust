<?php

namespace FlowCatalyst\Generated\Model;

class SubscriptionResponse
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
    protected $applicationCode;
    /**
     * @var string|null
     */
    protected $clientId;
    /**
     * @var string|null
     */
    protected $clientIdentifier;
    /**
     * @var bool|null
     */
    protected $clientScoped;
    /**
     * @var string|null
     */
    protected $code;
    /**
     * @var string|null
     */
    protected $connectionId;
    /**
     * @var \DateTime|null
     */
    protected $createdAt;
    /**
     * @var string|null
     */
    protected $createdBy;
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
    protected $dispatchPoolCode;
    /**
     * @var string|null
     */
    protected $dispatchPoolId;
    /**
     * @var string|null
     */
    protected $endpoint;
    /**
     * @var list<EventTypeBindingDTO>|null
     */
    protected $eventTypes;
    /**
     * @var string|null
     */
    protected $id;
    /**
     * @var int|null
     */
    protected $maxAgeSeconds;
    /**
     * @var int|null
     */
    protected $maxRetries;
    /**
     * @var string|null
     */
    protected $mode;
    /**
     * @var string|null
     */
    protected $name;
    /**
     * @var string|null
     */
    protected $queue;
    /**
     * @var int|null
     */
    protected $sequence;
    /**
     * @var string|null
     */
    protected $serviceAccountId;
    /**
     * @var string|null
     */
    protected $source;
    /**
     * @var string|null
     */
    protected $status;
    /**
     * @var int|null
     */
    protected $timeoutSeconds;
    /**
     * @var \DateTime|null
     */
    protected $updatedAt;
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
    public function getApplicationCode(): ?string
    {
        return $this->applicationCode;
    }
    /**
     * @param string|null $applicationCode
     *
     * @return self
     */
    public function setApplicationCode(?string $applicationCode): self
    {
        $this->initialized['applicationCode'] = true;
        $this->applicationCode = $applicationCode;
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
    public function getClientIdentifier(): ?string
    {
        return $this->clientIdentifier;
    }
    /**
     * @param string|null $clientIdentifier
     *
     * @return self
     */
    public function setClientIdentifier(?string $clientIdentifier): self
    {
        $this->initialized['clientIdentifier'] = true;
        $this->clientIdentifier = $clientIdentifier;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getClientScoped(): ?bool
    {
        return $this->clientScoped;
    }
    /**
     * @param bool|null $clientScoped
     *
     * @return self
     */
    public function setClientScoped(?bool $clientScoped): self
    {
        $this->initialized['clientScoped'] = true;
        $this->clientScoped = $clientScoped;
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
     * @return \DateTime|null
     */
    public function getCreatedAt(): ?\DateTime
    {
        return $this->createdAt;
    }
    /**
     * @param \DateTime|null $createdAt
     *
     * @return self
     */
    public function setCreatedAt(?\DateTime $createdAt): self
    {
        $this->initialized['createdAt'] = true;
        $this->createdAt = $createdAt;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getCreatedBy(): ?string
    {
        return $this->createdBy;
    }
    /**
     * @param string|null $createdBy
     *
     * @return self
     */
    public function setCreatedBy(?string $createdBy): self
    {
        $this->initialized['createdBy'] = true;
        $this->createdBy = $createdBy;
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
    public function getDispatchPoolCode(): ?string
    {
        return $this->dispatchPoolCode;
    }
    /**
     * @param string|null $dispatchPoolCode
     *
     * @return self
     */
    public function setDispatchPoolCode(?string $dispatchPoolCode): self
    {
        $this->initialized['dispatchPoolCode'] = true;
        $this->dispatchPoolCode = $dispatchPoolCode;
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
     * @return string|null
     */
    public function getEndpoint(): ?string
    {
        return $this->endpoint;
    }
    /**
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
     * @return string|null
     */
    public function getId(): ?string
    {
        return $this->id;
    }
    /**
     * @param string|null $id
     *
     * @return self
     */
    public function setId(?string $id): self
    {
        $this->initialized['id'] = true;
        $this->id = $id;
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
     * @return string|null
     */
    public function getMode(): ?string
    {
        return $this->mode;
    }
    /**
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
     * @return string|null
     */
    public function getQueue(): ?string
    {
        return $this->queue;
    }
    /**
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
     * @return int|null
     */
    public function getSequence(): ?int
    {
        return $this->sequence;
    }
    /**
     * @param int|null $sequence
     *
     * @return self
     */
    public function setSequence(?int $sequence): self
    {
        $this->initialized['sequence'] = true;
        $this->sequence = $sequence;
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
     * @return string|null
     */
    public function getSource(): ?string
    {
        return $this->source;
    }
    /**
     * @param string|null $source
     *
     * @return self
     */
    public function setSource(?string $source): self
    {
        $this->initialized['source'] = true;
        $this->source = $source;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getStatus(): ?string
    {
        return $this->status;
    }
    /**
     * @param string|null $status
     *
     * @return self
     */
    public function setStatus(?string $status): self
    {
        $this->initialized['status'] = true;
        $this->status = $status;
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
    /**
     * @return \DateTime|null
     */
    public function getUpdatedAt(): ?\DateTime
    {
        return $this->updatedAt;
    }
    /**
     * @param \DateTime|null $updatedAt
     *
     * @return self
     */
    public function setUpdatedAt(?\DateTime $updatedAt): self
    {
        $this->initialized['updatedAt'] = true;
        $this->updatedAt = $updatedAt;
        return $this;
    }
}