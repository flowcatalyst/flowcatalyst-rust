<?php

namespace FlowCatalyst\Generated\Model;

class IdpRoleMappingResponse
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
    protected $idpRoleName;
    /**
     * @var string|null
     */
    protected $idpType;
    /**
     * @var string|null
     */
    protected $platformRoleName;
    /**
     * @var \DateTime|null
     */
    protected $updatedAt;
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
    public function getIdpRoleName(): ?string
    {
        return $this->idpRoleName;
    }
    /**
     * @param string|null $idpRoleName
     *
     * @return self
     */
    public function setIdpRoleName(?string $idpRoleName): self
    {
        $this->initialized['idpRoleName'] = true;
        $this->idpRoleName = $idpRoleName;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getIdpType(): ?string
    {
        return $this->idpType;
    }
    /**
     * @param string|null $idpType
     *
     * @return self
     */
    public function setIdpType(?string $idpType): self
    {
        $this->initialized['idpType'] = true;
        $this->idpType = $idpType;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getPlatformRoleName(): ?string
    {
        return $this->platformRoleName;
    }
    /**
     * @param string|null $platformRoleName
     *
     * @return self
     */
    public function setPlatformRoleName(?string $platformRoleName): self
    {
        $this->initialized['platformRoleName'] = true;
        $this->platformRoleName = $platformRoleName;
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