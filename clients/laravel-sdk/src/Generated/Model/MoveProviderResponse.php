<?php

namespace FlowCatalyst\Generated\Model;

class MoveProviderResponse
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
    protected $emailDomain;
    /**
     * @var string|null
     */
    protected $fromIdentityProviderId;
    /**
     * @var string|null
     */
    protected $mappingId;
    /**
     * @var string|null
     */
    protected $toIdentityProviderId;
    /**
     * OIDC-provisioned users converted back to internal auth (0 when moving to an OIDC provider)
     *
     * @var int|null
     */
    protected $usersReset;
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
     * @return string|null
     */
    public function getFromIdentityProviderId(): ?string
    {
        return $this->fromIdentityProviderId;
    }
    /**
     * @param string|null $fromIdentityProviderId
     *
     * @return self
     */
    public function setFromIdentityProviderId(?string $fromIdentityProviderId): self
    {
        $this->initialized['fromIdentityProviderId'] = true;
        $this->fromIdentityProviderId = $fromIdentityProviderId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getMappingId(): ?string
    {
        return $this->mappingId;
    }
    /**
     * @param string|null $mappingId
     *
     * @return self
     */
    public function setMappingId(?string $mappingId): self
    {
        $this->initialized['mappingId'] = true;
        $this->mappingId = $mappingId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getToIdentityProviderId(): ?string
    {
        return $this->toIdentityProviderId;
    }
    /**
     * @param string|null $toIdentityProviderId
     *
     * @return self
     */
    public function setToIdentityProviderId(?string $toIdentityProviderId): self
    {
        $this->initialized['toIdentityProviderId'] = true;
        $this->toIdentityProviderId = $toIdentityProviderId;
        return $this;
    }
    /**
     * OIDC-provisioned users converted back to internal auth (0 when moving to an OIDC provider)
     *
     * @return int|null
     */
    public function getUsersReset(): ?int
    {
        return $this->usersReset;
    }
    /**
     * OIDC-provisioned users converted back to internal auth (0 when moving to an OIDC provider)
     *
     * @param int|null $usersReset
     *
     * @return self
     */
    public function setUsersReset(?int $usersReset): self
    {
        $this->initialized['usersReset'] = true;
        $this->usersReset = $usersReset;
        return $this;
    }
}