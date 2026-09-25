<?php

namespace FlowCatalyst\Generated\Model;

class LoginAttemptResponse
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
    protected $attemptType;
    /**
     * @var \DateTime|null
     */
    protected $attemptedAt;
    /**
     * @var string|null
     */
    protected $failureReason;
    /**
     * @var string|null
     */
    protected $id;
    /**
     * @var string|null
     */
    protected $identifier;
    /**
     * @var string|null
     */
    protected $ipAddress;
    /**
     * @var string|null
     */
    protected $outcome;
    /**
     * @var string|null
     */
    protected $principalId;
    /**
     * @var string|null
     */
    protected $userAgent;
    /**
     * @return string|null
     */
    public function getAttemptType(): ?string
    {
        return $this->attemptType;
    }
    /**
     * @param string|null $attemptType
     *
     * @return self
     */
    public function setAttemptType(?string $attemptType): self
    {
        $this->initialized['attemptType'] = true;
        $this->attemptType = $attemptType;
        return $this;
    }
    /**
     * @return \DateTime|null
     */
    public function getAttemptedAt(): ?\DateTime
    {
        return $this->attemptedAt;
    }
    /**
     * @param \DateTime|null $attemptedAt
     *
     * @return self
     */
    public function setAttemptedAt(?\DateTime $attemptedAt): self
    {
        $this->initialized['attemptedAt'] = true;
        $this->attemptedAt = $attemptedAt;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getFailureReason(): ?string
    {
        return $this->failureReason;
    }
    /**
     * @param string|null $failureReason
     *
     * @return self
     */
    public function setFailureReason(?string $failureReason): self
    {
        $this->initialized['failureReason'] = true;
        $this->failureReason = $failureReason;
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
    public function getIdentifier(): ?string
    {
        return $this->identifier;
    }
    /**
     * @param string|null $identifier
     *
     * @return self
     */
    public function setIdentifier(?string $identifier): self
    {
        $this->initialized['identifier'] = true;
        $this->identifier = $identifier;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getIpAddress(): ?string
    {
        return $this->ipAddress;
    }
    /**
     * @param string|null $ipAddress
     *
     * @return self
     */
    public function setIpAddress(?string $ipAddress): self
    {
        $this->initialized['ipAddress'] = true;
        $this->ipAddress = $ipAddress;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getOutcome(): ?string
    {
        return $this->outcome;
    }
    /**
     * @param string|null $outcome
     *
     * @return self
     */
    public function setOutcome(?string $outcome): self
    {
        $this->initialized['outcome'] = true;
        $this->outcome = $outcome;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getPrincipalId(): ?string
    {
        return $this->principalId;
    }
    /**
     * @param string|null $principalId
     *
     * @return self
     */
    public function setPrincipalId(?string $principalId): self
    {
        $this->initialized['principalId'] = true;
        $this->principalId = $principalId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getUserAgent(): ?string
    {
        return $this->userAgent;
    }
    /**
     * @param string|null $userAgent
     *
     * @return self
     */
    public function setUserAgent(?string $userAgent): self
    {
        $this->initialized['userAgent'] = true;
        $this->userAgent = $userAgent;
        return $this;
    }
}