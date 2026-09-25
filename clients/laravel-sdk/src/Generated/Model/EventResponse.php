<?php

namespace FlowCatalyst\Generated\Model;

class EventResponse
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
    protected $aggregate;
    /**
     * @var string|null
     */
    protected $application;
    /**
     * @var string|null
     */
    protected $causationId;
    /**
     * @var string|null
     */
    protected $clientId;
    /**
     * @var list<ContextEntryDTO>|null
     */
    protected $contextData;
    /**
     * @var string|null
     */
    protected $correlationId;
    /**
     * @var \DateTime|null
     */
    protected $createdAt;
    /**
     * @var mixed|null
     */
    protected $data;
    /**
     * @var string|null
     */
    protected $deduplicationId;
    /**
     * @var string|null
     */
    protected $id;
    /**
     * @var string|null
     */
    protected $messageGroup;
    /**
     * @var \DateTime|null
     */
    protected $projectedAt;
    /**
     * @var string|null
     */
    protected $source;
    /**
     * @var string|null
     */
    protected $specVersion;
    /**
     * @var string|null
     */
    protected $subdomain;
    /**
     * @var string|null
     */
    protected $subject;
    /**
     * @var \DateTime|null
     */
    protected $time;
    /**
     * @var string|null
     */
    protected $type;
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
    public function getAggregate(): ?string
    {
        return $this->aggregate;
    }
    /**
     * @param string|null $aggregate
     *
     * @return self
     */
    public function setAggregate(?string $aggregate): self
    {
        $this->initialized['aggregate'] = true;
        $this->aggregate = $aggregate;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getApplication(): ?string
    {
        return $this->application;
    }
    /**
     * @param string|null $application
     *
     * @return self
     */
    public function setApplication(?string $application): self
    {
        $this->initialized['application'] = true;
        $this->application = $application;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getCausationId(): ?string
    {
        return $this->causationId;
    }
    /**
     * @param string|null $causationId
     *
     * @return self
     */
    public function setCausationId(?string $causationId): self
    {
        $this->initialized['causationId'] = true;
        $this->causationId = $causationId;
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
     * @return list<ContextEntryDTO>|null
     */
    public function getContextData(): ?array
    {
        return $this->contextData;
    }
    /**
     * @param list<ContextEntryDTO>|null $contextData
     *
     * @return self
     */
    public function setContextData(?array $contextData): self
    {
        $this->initialized['contextData'] = true;
        $this->contextData = $contextData;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getCorrelationId(): ?string
    {
        return $this->correlationId;
    }
    /**
     * @param string|null $correlationId
     *
     * @return self
     */
    public function setCorrelationId(?string $correlationId): self
    {
        $this->initialized['correlationId'] = true;
        $this->correlationId = $correlationId;
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
     * @return mixed
     */
    public function getData()
    {
        return $this->data;
    }
    /**
     * @param mixed $data
     *
     * @return self
     */
    public function setData($data): self
    {
        $this->initialized['data'] = true;
        $this->data = $data;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getDeduplicationId(): ?string
    {
        return $this->deduplicationId;
    }
    /**
     * @param string|null $deduplicationId
     *
     * @return self
     */
    public function setDeduplicationId(?string $deduplicationId): self
    {
        $this->initialized['deduplicationId'] = true;
        $this->deduplicationId = $deduplicationId;
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
     * @return string|null
     */
    public function getMessageGroup(): ?string
    {
        return $this->messageGroup;
    }
    /**
     * @param string|null $messageGroup
     *
     * @return self
     */
    public function setMessageGroup(?string $messageGroup): self
    {
        $this->initialized['messageGroup'] = true;
        $this->messageGroup = $messageGroup;
        return $this;
    }
    /**
     * @return \DateTime|null
     */
    public function getProjectedAt(): ?\DateTime
    {
        return $this->projectedAt;
    }
    /**
     * @param \DateTime|null $projectedAt
     *
     * @return self
     */
    public function setProjectedAt(?\DateTime $projectedAt): self
    {
        $this->initialized['projectedAt'] = true;
        $this->projectedAt = $projectedAt;
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
    public function getSpecVersion(): ?string
    {
        return $this->specVersion;
    }
    /**
     * @param string|null $specVersion
     *
     * @return self
     */
    public function setSpecVersion(?string $specVersion): self
    {
        $this->initialized['specVersion'] = true;
        $this->specVersion = $specVersion;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getSubdomain(): ?string
    {
        return $this->subdomain;
    }
    /**
     * @param string|null $subdomain
     *
     * @return self
     */
    public function setSubdomain(?string $subdomain): self
    {
        $this->initialized['subdomain'] = true;
        $this->subdomain = $subdomain;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getSubject(): ?string
    {
        return $this->subject;
    }
    /**
     * @param string|null $subject
     *
     * @return self
     */
    public function setSubject(?string $subject): self
    {
        $this->initialized['subject'] = true;
        $this->subject = $subject;
        return $this;
    }
    /**
     * @return \DateTime|null
     */
    public function getTime(): ?\DateTime
    {
        return $this->time;
    }
    /**
     * @param \DateTime|null $time
     *
     * @return self
     */
    public function setTime(?\DateTime $time): self
    {
        $this->initialized['time'] = true;
        $this->time = $time;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getType(): ?string
    {
        return $this->type;
    }
    /**
     * @param string|null $type
     *
     * @return self
     */
    public function setType(?string $type): self
    {
        $this->initialized['type'] = true;
        $this->type = $type;
        return $this;
    }
}