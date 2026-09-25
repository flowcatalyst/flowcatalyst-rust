<?php

namespace FlowCatalyst\Generated\Model;

class AccessResponse
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
    protected $applicationCode;
    /**
     * @var bool|null
     */
    protected $canRead;
    /**
     * @var bool|null
     */
    protected $canWrite;
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
    protected $roleCode;
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
     * @return bool|null
     */
    public function getCanRead(): ?bool
    {
        return $this->canRead;
    }
    /**
     * @param bool|null $canRead
     *
     * @return self
     */
    public function setCanRead(?bool $canRead): self
    {
        $this->initialized['canRead'] = true;
        $this->canRead = $canRead;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getCanWrite(): ?bool
    {
        return $this->canWrite;
    }
    /**
     * @param bool|null $canWrite
     *
     * @return self
     */
    public function setCanWrite(?bool $canWrite): self
    {
        $this->initialized['canWrite'] = true;
        $this->canWrite = $canWrite;
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
    public function getRoleCode(): ?string
    {
        return $this->roleCode;
    }
    /**
     * @param string|null $roleCode
     *
     * @return self
     */
    public function setRoleCode(?string $roleCode): self
    {
        $this->initialized['roleCode'] = true;
        $this->roleCode = $roleCode;
        return $this;
    }
}