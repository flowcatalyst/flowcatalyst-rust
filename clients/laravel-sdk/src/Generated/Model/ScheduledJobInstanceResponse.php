<?php

namespace FlowCatalyst\Generated\Model;

class ScheduledJobInstanceResponse
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
     * @var \DateTime|null
     */
    protected $completedAt;
    /**
     * @var mixed|null
     */
    protected $completionResult;
    /**
     * @var string|null
     */
    protected $completionStatus;
    /**
     * @var string|null
     */
    protected $correlationId;
    /**
     * @var \DateTime|null
     */
    protected $createdAt;
    /**
     * @var \DateTime|null
     */
    protected $deliveredAt;
    /**
     * @var int|null
     */
    protected $deliveryAttempts;
    /**
     * @var string|null
     */
    protected $deliveryError;
    /**
     * @var \DateTime|null
     */
    protected $firedAt;
    /**
     * @var string|null
     */
    protected $id;
    /**
     * @var string|null
     */
    protected $jobCode;
    /**
     * @var \DateTime|null
     */
    protected $scheduledFor;
    /**
     * @var string|null
     */
    protected $scheduledJobId;
    /**
     * @var string|null
     */
    protected $status;
    /**
     * @var string|null
     */
    protected $triggerKind;
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
     * @return \DateTime|null
     */
    public function getCompletedAt(): ?\DateTime
    {
        return $this->completedAt;
    }
    /**
     * @param \DateTime|null $completedAt
     *
     * @return self
     */
    public function setCompletedAt(?\DateTime $completedAt): self
    {
        $this->initialized['completedAt'] = true;
        $this->completedAt = $completedAt;
        return $this;
    }
    /**
     * @return mixed
     */
    public function getCompletionResult()
    {
        return $this->completionResult;
    }
    /**
     * @param mixed $completionResult
     *
     * @return self
     */
    public function setCompletionResult($completionResult): self
    {
        $this->initialized['completionResult'] = true;
        $this->completionResult = $completionResult;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getCompletionStatus(): ?string
    {
        return $this->completionStatus;
    }
    /**
     * @param string|null $completionStatus
     *
     * @return self
     */
    public function setCompletionStatus(?string $completionStatus): self
    {
        $this->initialized['completionStatus'] = true;
        $this->completionStatus = $completionStatus;
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
     * @return \DateTime|null
     */
    public function getDeliveredAt(): ?\DateTime
    {
        return $this->deliveredAt;
    }
    /**
     * @param \DateTime|null $deliveredAt
     *
     * @return self
     */
    public function setDeliveredAt(?\DateTime $deliveredAt): self
    {
        $this->initialized['deliveredAt'] = true;
        $this->deliveredAt = $deliveredAt;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getDeliveryAttempts(): ?int
    {
        return $this->deliveryAttempts;
    }
    /**
     * @param int|null $deliveryAttempts
     *
     * @return self
     */
    public function setDeliveryAttempts(?int $deliveryAttempts): self
    {
        $this->initialized['deliveryAttempts'] = true;
        $this->deliveryAttempts = $deliveryAttempts;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getDeliveryError(): ?string
    {
        return $this->deliveryError;
    }
    /**
     * @param string|null $deliveryError
     *
     * @return self
     */
    public function setDeliveryError(?string $deliveryError): self
    {
        $this->initialized['deliveryError'] = true;
        $this->deliveryError = $deliveryError;
        return $this;
    }
    /**
     * @return \DateTime|null
     */
    public function getFiredAt(): ?\DateTime
    {
        return $this->firedAt;
    }
    /**
     * @param \DateTime|null $firedAt
     *
     * @return self
     */
    public function setFiredAt(?\DateTime $firedAt): self
    {
        $this->initialized['firedAt'] = true;
        $this->firedAt = $firedAt;
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
    public function getJobCode(): ?string
    {
        return $this->jobCode;
    }
    /**
     * @param string|null $jobCode
     *
     * @return self
     */
    public function setJobCode(?string $jobCode): self
    {
        $this->initialized['jobCode'] = true;
        $this->jobCode = $jobCode;
        return $this;
    }
    /**
     * @return \DateTime|null
     */
    public function getScheduledFor(): ?\DateTime
    {
        return $this->scheduledFor;
    }
    /**
     * @param \DateTime|null $scheduledFor
     *
     * @return self
     */
    public function setScheduledFor(?\DateTime $scheduledFor): self
    {
        $this->initialized['scheduledFor'] = true;
        $this->scheduledFor = $scheduledFor;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getScheduledJobId(): ?string
    {
        return $this->scheduledJobId;
    }
    /**
     * @param string|null $scheduledJobId
     *
     * @return self
     */
    public function setScheduledJobId(?string $scheduledJobId): self
    {
        $this->initialized['scheduledJobId'] = true;
        $this->scheduledJobId = $scheduledJobId;
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
    public function getTriggerKind(): ?string
    {
        return $this->triggerKind;
    }
    /**
     * @param string|null $triggerKind
     *
     * @return self
     */
    public function setTriggerKind(?string $triggerKind): self
    {
        $this->initialized['triggerKind'] = true;
        $this->triggerKind = $triggerKind;
        return $this;
    }
}