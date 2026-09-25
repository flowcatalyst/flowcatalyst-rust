<?php

namespace FlowCatalyst\Generated\Model;

class RawDispatchJobResponse
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
    protected $attemptCount;
    /**
     * @var int|null
     */
    protected $attemptHistoryCount;
    /**
     * @var string|null
     */
    protected $clientId;
    /**
     * @var string|null
     */
    protected $code;
    /**
     * @var \DateTime|null
     */
    protected $completedAt;
    /**
     * @var string|null
     */
    protected $correlationId;
    /**
     * @var \DateTime|null
     */
    protected $createdAt;
    /**
     * @var string|null
     */
    protected $dispatchPoolId;
    /**
     * @var string|null
     */
    protected $eventId;
    /**
     * @var string|null
     */
    protected $externalId;
    /**
     * @var string|null
     */
    protected $id;
    /**
     * @var string|null
     */
    protected $idempotencyKey;
    /**
     * @var string|null
     */
    protected $kind;
    /**
     * @var string|null
     */
    protected $lastError;
    /**
     * @var int|null
     */
    protected $maxRetries;
    /**
     * @var string|null
     */
    protected $messageGroup;
    /**
     * @var string|null
     */
    protected $mode;
    /**
     * @var string|null
     */
    protected $payloadContentType;
    /**
     * @var int|null
     */
    protected $payloadLength;
    /**
     * @var string|null
     */
    protected $protocol;
    /**
     * @var string|null
     */
    protected $retryStrategy;
    /**
     * @var \DateTime|null
     */
    protected $scheduledFor;
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
     * @var string|null
     */
    protected $subject;
    /**
     * @var string|null
     */
    protected $subscriptionId;
    /**
     * @var string|null
     */
    protected $targetUrl;
    /**
     * @var int|null
     */
    protected $timeoutSeconds;
    /**
     * @var \DateTime|null
     */
    protected $updatedAt;
    /**
     * @return int|null
     */
    public function getAttemptCount(): ?int
    {
        return $this->attemptCount;
    }
    /**
     * @param int|null $attemptCount
     *
     * @return self
     */
    public function setAttemptCount(?int $attemptCount): self
    {
        $this->initialized['attemptCount'] = true;
        $this->attemptCount = $attemptCount;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getAttemptHistoryCount(): ?int
    {
        return $this->attemptHistoryCount;
    }
    /**
     * @param int|null $attemptHistoryCount
     *
     * @return self
     */
    public function setAttemptHistoryCount(?int $attemptHistoryCount): self
    {
        $this->initialized['attemptHistoryCount'] = true;
        $this->attemptHistoryCount = $attemptHistoryCount;
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
    public function getEventId(): ?string
    {
        return $this->eventId;
    }
    /**
     * @param string|null $eventId
     *
     * @return self
     */
    public function setEventId(?string $eventId): self
    {
        $this->initialized['eventId'] = true;
        $this->eventId = $eventId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getExternalId(): ?string
    {
        return $this->externalId;
    }
    /**
     * @param string|null $externalId
     *
     * @return self
     */
    public function setExternalId(?string $externalId): self
    {
        $this->initialized['externalId'] = true;
        $this->externalId = $externalId;
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
    public function getIdempotencyKey(): ?string
    {
        return $this->idempotencyKey;
    }
    /**
     * @param string|null $idempotencyKey
     *
     * @return self
     */
    public function setIdempotencyKey(?string $idempotencyKey): self
    {
        $this->initialized['idempotencyKey'] = true;
        $this->idempotencyKey = $idempotencyKey;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getKind(): ?string
    {
        return $this->kind;
    }
    /**
     * @param string|null $kind
     *
     * @return self
     */
    public function setKind(?string $kind): self
    {
        $this->initialized['kind'] = true;
        $this->kind = $kind;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getLastError(): ?string
    {
        return $this->lastError;
    }
    /**
     * @param string|null $lastError
     *
     * @return self
     */
    public function setLastError(?string $lastError): self
    {
        $this->initialized['lastError'] = true;
        $this->lastError = $lastError;
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
    public function getPayloadContentType(): ?string
    {
        return $this->payloadContentType;
    }
    /**
     * @param string|null $payloadContentType
     *
     * @return self
     */
    public function setPayloadContentType(?string $payloadContentType): self
    {
        $this->initialized['payloadContentType'] = true;
        $this->payloadContentType = $payloadContentType;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getPayloadLength(): ?int
    {
        return $this->payloadLength;
    }
    /**
     * @param int|null $payloadLength
     *
     * @return self
     */
    public function setPayloadLength(?int $payloadLength): self
    {
        $this->initialized['payloadLength'] = true;
        $this->payloadLength = $payloadLength;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getProtocol(): ?string
    {
        return $this->protocol;
    }
    /**
     * @param string|null $protocol
     *
     * @return self
     */
    public function setProtocol(?string $protocol): self
    {
        $this->initialized['protocol'] = true;
        $this->protocol = $protocol;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getRetryStrategy(): ?string
    {
        return $this->retryStrategy;
    }
    /**
     * @param string|null $retryStrategy
     *
     * @return self
     */
    public function setRetryStrategy(?string $retryStrategy): self
    {
        $this->initialized['retryStrategy'] = true;
        $this->retryStrategy = $retryStrategy;
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
     * @return string|null
     */
    public function getSubscriptionId(): ?string
    {
        return $this->subscriptionId;
    }
    /**
     * @param string|null $subscriptionId
     *
     * @return self
     */
    public function setSubscriptionId(?string $subscriptionId): self
    {
        $this->initialized['subscriptionId'] = true;
        $this->subscriptionId = $subscriptionId;
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