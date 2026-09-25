<?php

namespace FlowCatalyst\Generated\Model;

class ScheduledJobInstanceLogResponse
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
    protected $clientId;
    /**
     * @var \DateTime|null
     */
    protected $createdAt;
    /**
     * @var string|null
     */
    protected $id;
    /**
     * @var string|null
     */
    protected $instanceId;
    /**
     * @var string|null
     */
    protected $level;
    /**
     * @var string|null
     */
    protected $message;
    /**
     * @var mixed|null
     */
    protected $metadata;
    /**
     * @var string|null
     */
    protected $scheduledJobId;
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
    public function getInstanceId(): ?string
    {
        return $this->instanceId;
    }
    /**
     * @param string|null $instanceId
     *
     * @return self
     */
    public function setInstanceId(?string $instanceId): self
    {
        $this->initialized['instanceId'] = true;
        $this->instanceId = $instanceId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getLevel(): ?string
    {
        return $this->level;
    }
    /**
     * @param string|null $level
     *
     * @return self
     */
    public function setLevel(?string $level): self
    {
        $this->initialized['level'] = true;
        $this->level = $level;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getMessage(): ?string
    {
        return $this->message;
    }
    /**
     * @param string|null $message
     *
     * @return self
     */
    public function setMessage(?string $message): self
    {
        $this->initialized['message'] = true;
        $this->message = $message;
        return $this;
    }
    /**
     * @return mixed
     */
    public function getMetadata()
    {
        return $this->metadata;
    }
    /**
     * @param mixed $metadata
     *
     * @return self
     */
    public function setMetadata($metadata): self
    {
        $this->initialized['metadata'] = true;
        $this->metadata = $metadata;
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
}