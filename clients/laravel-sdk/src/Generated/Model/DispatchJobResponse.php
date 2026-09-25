<?php

namespace FlowCatalyst\Generated\Model;

class DispatchJobResponse
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
     * @var int|null
     */
    protected $attemptCount;
    /**
     * @var list<AttemptDTO>|null
     */
    protected $attempts;
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
     * @var bool|null
     */
    protected $dataOnly;
    /**
     * @var string|null
     */
    protected $dispatchPoolId;
    /**
     * @var int|null
     */
    protected $durationMillis;
    /**
     * @var string|null
     */
    protected $eventId;
    /**
     * @var \DateTime|null
     */
    protected $expiresAt;
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
     * @var \DateTime|null
     */
    protected $lastAttemptAt;
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
     * @var list<MetadataDTO>|null
     */
    protected $metadata;
    /**
     * @var string|null
     */
    protected $mode;
    /**
     * @var string|null
     */
    protected $payload;
    /**
     * @var string|null
     */
    protected $payloadContentType;
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
     * @var string|null
     */
    protected $schemaId;
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
     * @return list<AttemptDTO>|null
     */
    public function getAttempts(): ?array
    {
        return $this->attempts;
    }
    /**
     * @param list<AttemptDTO>|null $attempts
     *
     * @return self
     */
    public function setAttempts(?array $attempts): self
    {
        $this->initialized['attempts'] = true;
        $this->attempts = $attempts;
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
     * @return int|null
     */
    public function getDurationMillis(): ?int
    {
        return $this->durationMillis;
    }
    /**
     * @param int|null $durationMillis
     *
     * @return self
     */
    public function setDurationMillis(?int $durationMillis): self
    {
        $this->initialized['durationMillis'] = true;
        $this->durationMillis = $durationMillis;
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
     * @return \DateTime|null
     */
    public function getExpiresAt(): ?\DateTime
    {
        return $this->expiresAt;
    }
    /**
     * @param \DateTime|null $expiresAt
     *
     * @return self
     */
    public function setExpiresAt(?\DateTime $expiresAt): self
    {
        $this->initialized['expiresAt'] = true;
        $this->expiresAt = $expiresAt;
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
     * @return \DateTime|null
     */
    public function getLastAttemptAt(): ?\DateTime
    {
        return $this->lastAttemptAt;
    }
    /**
     * @param \DateTime|null $lastAttemptAt
     *
     * @return self
     */
    public function setLastAttemptAt(?\DateTime $lastAttemptAt): self
    {
        $this->initialized['lastAttemptAt'] = true;
        $this->lastAttemptAt = $lastAttemptAt;
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
     * @return list<MetadataDTO>|null
     */
    public function getMetadata(): ?array
    {
        return $this->metadata;
    }
    /**
     * @param list<MetadataDTO>|null $metadata
     *
     * @return self
     */
    public function setMetadata(?array $metadata): self
    {
        $this->initialized['metadata'] = true;
        $this->metadata = $metadata;
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
    public function getPayload(): ?string
    {
        return $this->payload;
    }
    /**
     * @param string|null $payload
     *
     * @return self
     */
    public function setPayload(?string $payload): self
    {
        $this->initialized['payload'] = true;
        $this->payload = $payload;
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
     * @return string|null
     */
    public function getSchemaId(): ?string
    {
        return $this->schemaId;
    }
    /**
     * @param string|null $schemaId
     *
     * @return self
     */
    public function setSchemaId(?string $schemaId): self
    {
        $this->initialized['schemaId'] = true;
        $this->schemaId = $schemaId;
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