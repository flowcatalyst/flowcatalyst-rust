<?php

namespace FlowCatalyst\Generated\Model;

class ScheduledJobResponse
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
    protected $applicationId;
    /**
     * @var string|null
     */
    protected $clientId;
    /**
     * @var string|null
     */
    protected $code;
    /**
     * @var bool|null
     */
    protected $concurrent;
    /**
     * @var \DateTime|null
     */
    protected $createdAt;
    /**
     * @var string|null
     */
    protected $createdBy;
    /**
     * @var list<string>|null
     */
    protected $crons;
    /**
     * @var int|null
     */
    protected $deliveryMaxAttempts;
    /**
     * @var string|null
     */
    protected $description;
    /**
     * @var bool|null
     */
    protected $hasActiveInstance;
    /**
     * @var string|null
     */
    protected $id;
    /**
     * @var \DateTime|null
     */
    protected $lastFiredAt;
    /**
     * @var string|null
     */
    protected $name;
    /**
     * @var mixed|null
     */
    protected $payload;
    /**
     * @var string|null
     */
    protected $status;
    /**
     * @var string|null
     */
    protected $targetUrl;
    /**
     * @var int|null
     */
    protected $timeoutSeconds;
    /**
     * @var string|null
     */
    protected $timezone;
    /**
     * @var bool|null
     */
    protected $tracksCompletion;
    /**
     * @var \DateTime|null
     */
    protected $updatedAt;
    /**
     * @var string|null
     */
    protected $updatedBy;
    /**
     * @var int|null
     */
    protected $version;
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
    public function getApplicationId(): ?string
    {
        return $this->applicationId;
    }
    /**
     * @param string|null $applicationId
     *
     * @return self
     */
    public function setApplicationId(?string $applicationId): self
    {
        $this->initialized['applicationId'] = true;
        $this->applicationId = $applicationId;
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
     * @return bool|null
     */
    public function getConcurrent(): ?bool
    {
        return $this->concurrent;
    }
    /**
     * @param bool|null $concurrent
     *
     * @return self
     */
    public function setConcurrent(?bool $concurrent): self
    {
        $this->initialized['concurrent'] = true;
        $this->concurrent = $concurrent;
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
     * @return list<string>|null
     */
    public function getCrons(): ?array
    {
        return $this->crons;
    }
    /**
     * @param list<string>|null $crons
     *
     * @return self
     */
    public function setCrons(?array $crons): self
    {
        $this->initialized['crons'] = true;
        $this->crons = $crons;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getDeliveryMaxAttempts(): ?int
    {
        return $this->deliveryMaxAttempts;
    }
    /**
     * @param int|null $deliveryMaxAttempts
     *
     * @return self
     */
    public function setDeliveryMaxAttempts(?int $deliveryMaxAttempts): self
    {
        $this->initialized['deliveryMaxAttempts'] = true;
        $this->deliveryMaxAttempts = $deliveryMaxAttempts;
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
     * @return bool|null
     */
    public function getHasActiveInstance(): ?bool
    {
        return $this->hasActiveInstance;
    }
    /**
     * @param bool|null $hasActiveInstance
     *
     * @return self
     */
    public function setHasActiveInstance(?bool $hasActiveInstance): self
    {
        $this->initialized['hasActiveInstance'] = true;
        $this->hasActiveInstance = $hasActiveInstance;
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
     * @return \DateTime|null
     */
    public function getLastFiredAt(): ?\DateTime
    {
        return $this->lastFiredAt;
    }
    /**
     * @param \DateTime|null $lastFiredAt
     *
     * @return self
     */
    public function setLastFiredAt(?\DateTime $lastFiredAt): self
    {
        $this->initialized['lastFiredAt'] = true;
        $this->lastFiredAt = $lastFiredAt;
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
     * @return mixed
     */
    public function getPayload()
    {
        return $this->payload;
    }
    /**
     * @param mixed $payload
     *
     * @return self
     */
    public function setPayload($payload): self
    {
        $this->initialized['payload'] = true;
        $this->payload = $payload;
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
     * @return string|null
     */
    public function getTargetUrl(): ?string
    {
        return $this->targetUrl;
    }
    /**
     * @param string|null $targetUrl
     *
     * @return self
     */
    public function setTargetUrl(?string $targetUrl): self
    {
        $this->initialized['targetUrl'] = true;
        $this->targetUrl = $targetUrl;
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
     * @return string|null
     */
    public function getTimezone(): ?string
    {
        return $this->timezone;
    }
    /**
     * @param string|null $timezone
     *
     * @return self
     */
    public function setTimezone(?string $timezone): self
    {
        $this->initialized['timezone'] = true;
        $this->timezone = $timezone;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getTracksCompletion(): ?bool
    {
        return $this->tracksCompletion;
    }
    /**
     * @param bool|null $tracksCompletion
     *
     * @return self
     */
    public function setTracksCompletion(?bool $tracksCompletion): self
    {
        $this->initialized['tracksCompletion'] = true;
        $this->tracksCompletion = $tracksCompletion;
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
    /**
     * @return string|null
     */
    public function getUpdatedBy(): ?string
    {
        return $this->updatedBy;
    }
    /**
     * @param string|null $updatedBy
     *
     * @return self
     */
    public function setUpdatedBy(?string $updatedBy): self
    {
        $this->initialized['updatedBy'] = true;
        $this->updatedBy = $updatedBy;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getVersion(): ?int
    {
        return $this->version;
    }
    /**
     * @param int|null $version
     *
     * @return self
     */
    public function setVersion(?int $version): self
    {
        $this->initialized['version'] = true;
        $this->version = $version;
        return $this;
    }
}