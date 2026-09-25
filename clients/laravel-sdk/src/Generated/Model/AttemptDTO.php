<?php

namespace FlowCatalyst\Generated\Model;

class AttemptDTO
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
    protected $attemptNumber;
    /**
     * @var \DateTime|null
     */
    protected $attemptedAt;
    /**
     * @var \DateTime|null
     */
    protected $completedAt;
    /**
     * @var int|null
     */
    protected $durationMillis;
    /**
     * @var string|null
     */
    protected $errorMessage;
    /**
     * @var string|null
     */
    protected $errorType;
    /**
     * @var string|null
     */
    protected $responseBody;
    /**
     * @var int|null
     */
    protected $responseCode;
    /**
     * @var bool|null
     */
    protected $success;
    /**
     * @return int|null
     */
    public function getAttemptNumber(): ?int
    {
        return $this->attemptNumber;
    }
    /**
     * @param int|null $attemptNumber
     *
     * @return self
     */
    public function setAttemptNumber(?int $attemptNumber): self
    {
        $this->initialized['attemptNumber'] = true;
        $this->attemptNumber = $attemptNumber;
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
    public function getErrorMessage(): ?string
    {
        return $this->errorMessage;
    }
    /**
     * @param string|null $errorMessage
     *
     * @return self
     */
    public function setErrorMessage(?string $errorMessage): self
    {
        $this->initialized['errorMessage'] = true;
        $this->errorMessage = $errorMessage;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getErrorType(): ?string
    {
        return $this->errorType;
    }
    /**
     * @param string|null $errorType
     *
     * @return self
     */
    public function setErrorType(?string $errorType): self
    {
        $this->initialized['errorType'] = true;
        $this->errorType = $errorType;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getResponseBody(): ?string
    {
        return $this->responseBody;
    }
    /**
     * @param string|null $responseBody
     *
     * @return self
     */
    public function setResponseBody(?string $responseBody): self
    {
        $this->initialized['responseBody'] = true;
        $this->responseBody = $responseBody;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getResponseCode(): ?int
    {
        return $this->responseCode;
    }
    /**
     * @param int|null $responseCode
     *
     * @return self
     */
    public function setResponseCode(?int $responseCode): self
    {
        $this->initialized['responseCode'] = true;
        $this->responseCode = $responseCode;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getSuccess(): ?bool
    {
        return $this->success;
    }
    /**
     * @param bool|null $success
     *
     * @return self
     */
    public function setSuccess(?bool $success): self
    {
        $this->initialized['success'] = true;
        $this->success = $success;
        return $this;
    }
}