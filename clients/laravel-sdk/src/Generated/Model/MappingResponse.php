<?php

namespace FlowCatalyst\Generated\Model;

class MappingResponse
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
     * @var list<string>|null
     */
    protected $additionalClientIds;
    /**
     * @var list<string>|null
     */
    protected $allowed2faMethods;
    /**
     * @var \DateTime|null
     */
    protected $createdAt;
    /**
     * @var string|null
     */
    protected $emailDomain;
    /**
     * @var list<string>|null
     */
    protected $grantedClientIds;
    /**
     * @var string|null
     */
    protected $id;
    /**
     * @var string|null
     */
    protected $identityProviderId;
    /**
     * @var string|null
     */
    protected $identityProviderName;
    /**
     * @var string|null
     */
    protected $primaryClientId;
    /**
     * @var int|null
     */
    protected $rememberDeviceDays;
    /**
     * @var bool|null
     */
    protected $rememberDeviceEnabled;
    /**
     * @var bool|null
     */
    protected $require2fa;
    /**
     * @var string|null
     */
    protected $requiredOidcTenantId;
    /**
     * @var string|null
     */
    protected $scopeType;
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
     * @return list<string>|null
     */
    public function getAdditionalClientIds(): ?array
    {
        return $this->additionalClientIds;
    }
    /**
     * @param list<string>|null $additionalClientIds
     *
     * @return self
     */
    public function setAdditionalClientIds(?array $additionalClientIds): self
    {
        $this->initialized['additionalClientIds'] = true;
        $this->additionalClientIds = $additionalClientIds;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getAllowed2faMethods(): ?array
    {
        return $this->allowed2faMethods;
    }
    /**
     * @param list<string>|null $allowed2faMethods
     *
     * @return self
     */
    public function setAllowed2faMethods(?array $allowed2faMethods): self
    {
        $this->initialized['allowed2faMethods'] = true;
        $this->allowed2faMethods = $allowed2faMethods;
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
    public function getEmailDomain(): ?string
    {
        return $this->emailDomain;
    }
    /**
     * @param string|null $emailDomain
     *
     * @return self
     */
    public function setEmailDomain(?string $emailDomain): self
    {
        $this->initialized['emailDomain'] = true;
        $this->emailDomain = $emailDomain;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getGrantedClientIds(): ?array
    {
        return $this->grantedClientIds;
    }
    /**
     * @param list<string>|null $grantedClientIds
     *
     * @return self
     */
    public function setGrantedClientIds(?array $grantedClientIds): self
    {
        $this->initialized['grantedClientIds'] = true;
        $this->grantedClientIds = $grantedClientIds;
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
    public function getIdentityProviderId(): ?string
    {
        return $this->identityProviderId;
    }
    /**
     * @param string|null $identityProviderId
     *
     * @return self
     */
    public function setIdentityProviderId(?string $identityProviderId): self
    {
        $this->initialized['identityProviderId'] = true;
        $this->identityProviderId = $identityProviderId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getIdentityProviderName(): ?string
    {
        return $this->identityProviderName;
    }
    /**
     * @param string|null $identityProviderName
     *
     * @return self
     */
    public function setIdentityProviderName(?string $identityProviderName): self
    {
        $this->initialized['identityProviderName'] = true;
        $this->identityProviderName = $identityProviderName;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getPrimaryClientId(): ?string
    {
        return $this->primaryClientId;
    }
    /**
     * @param string|null $primaryClientId
     *
     * @return self
     */
    public function setPrimaryClientId(?string $primaryClientId): self
    {
        $this->initialized['primaryClientId'] = true;
        $this->primaryClientId = $primaryClientId;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getRememberDeviceDays(): ?int
    {
        return $this->rememberDeviceDays;
    }
    /**
     * @param int|null $rememberDeviceDays
     *
     * @return self
     */
    public function setRememberDeviceDays(?int $rememberDeviceDays): self
    {
        $this->initialized['rememberDeviceDays'] = true;
        $this->rememberDeviceDays = $rememberDeviceDays;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getRememberDeviceEnabled(): ?bool
    {
        return $this->rememberDeviceEnabled;
    }
    /**
     * @param bool|null $rememberDeviceEnabled
     *
     * @return self
     */
    public function setRememberDeviceEnabled(?bool $rememberDeviceEnabled): self
    {
        $this->initialized['rememberDeviceEnabled'] = true;
        $this->rememberDeviceEnabled = $rememberDeviceEnabled;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getRequire2fa(): ?bool
    {
        return $this->require2fa;
    }
    /**
     * @param bool|null $require2fa
     *
     * @return self
     */
    public function setRequire2fa(?bool $require2fa): self
    {
        $this->initialized['require2fa'] = true;
        $this->require2fa = $require2fa;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getRequiredOidcTenantId(): ?string
    {
        return $this->requiredOidcTenantId;
    }
    /**
     * @param string|null $requiredOidcTenantId
     *
     * @return self
     */
    public function setRequiredOidcTenantId(?string $requiredOidcTenantId): self
    {
        $this->initialized['requiredOidcTenantId'] = true;
        $this->requiredOidcTenantId = $requiredOidcTenantId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getScopeType(): ?string
    {
        return $this->scopeType;
    }
    /**
     * @param string|null $scopeType
     *
     * @return self
     */
    public function setScopeType(?string $scopeType): self
    {
        $this->initialized['scopeType'] = true;
        $this->scopeType = $scopeType;
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