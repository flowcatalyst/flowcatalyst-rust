<?php

namespace FlowCatalyst\Generated\Model;

class IdentityProviderResponse
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
     * Domains currently routed to this provider (derived from email-domain mappings)
     *
     * @var list<string>|null
     */
    protected $allowedEmailDomains;
    /**
     * @var list<string>|null
     */
    protected $allowedRoleIds;
    /**
     * @var string|null
     */
    protected $code;
    /**
     * @var \DateTime|null
     */
    protected $createdAt;
    /**
     * @var bool|null
     */
    protected $hasClientSecret;
    /**
     * @var string|null
     */
    protected $id;
    /**
     * @var string|null
     */
    protected $name;
    /**
     * @var string|null
     */
    protected $oidcClientId;
    /**
     * @var string|null
     */
    protected $oidcIssuerPattern;
    /**
     * @var string|null
     */
    protected $oidcIssuerUrl;
    /**
     * @var bool|null
     */
    protected $oidcMultiTenant;
    /**
     * @var bool|null
     */
    protected $syncRolesFromIdp;
    /**
     * @var string|null
     */
    protected $type;
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
     * Domains currently routed to this provider (derived from email-domain mappings)
     *
     * @return list<string>|null
     */
    public function getAllowedEmailDomains(): ?array
    {
        return $this->allowedEmailDomains;
    }
    /**
     * Domains currently routed to this provider (derived from email-domain mappings)
     *
     * @param list<string>|null $allowedEmailDomains
     *
     * @return self
     */
    public function setAllowedEmailDomains(?array $allowedEmailDomains): self
    {
        $this->initialized['allowedEmailDomains'] = true;
        $this->allowedEmailDomains = $allowedEmailDomains;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getAllowedRoleIds(): ?array
    {
        return $this->allowedRoleIds;
    }
    /**
     * @param list<string>|null $allowedRoleIds
     *
     * @return self
     */
    public function setAllowedRoleIds(?array $allowedRoleIds): self
    {
        $this->initialized['allowedRoleIds'] = true;
        $this->allowedRoleIds = $allowedRoleIds;
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
    public function getHasClientSecret(): ?bool
    {
        return $this->hasClientSecret;
    }
    /**
     * @param bool|null $hasClientSecret
     *
     * @return self
     */
    public function setHasClientSecret(?bool $hasClientSecret): self
    {
        $this->initialized['hasClientSecret'] = true;
        $this->hasClientSecret = $hasClientSecret;
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
    public function getName(): ?string
    {
        return $this->name;
    }
    /**
     * @param string|null $name
     *
     * @return self
     */
    public function setName(?string $name): self
    {
        $this->initialized['name'] = true;
        $this->name = $name;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getOidcClientId(): ?string
    {
        return $this->oidcClientId;
    }
    /**
     * @param string|null $oidcClientId
     *
     * @return self
     */
    public function setOidcClientId(?string $oidcClientId): self
    {
        $this->initialized['oidcClientId'] = true;
        $this->oidcClientId = $oidcClientId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getOidcIssuerPattern(): ?string
    {
        return $this->oidcIssuerPattern;
    }
    /**
     * @param string|null $oidcIssuerPattern
     *
     * @return self
     */
    public function setOidcIssuerPattern(?string $oidcIssuerPattern): self
    {
        $this->initialized['oidcIssuerPattern'] = true;
        $this->oidcIssuerPattern = $oidcIssuerPattern;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getOidcIssuerUrl(): ?string
    {
        return $this->oidcIssuerUrl;
    }
    /**
     * @param string|null $oidcIssuerUrl
     *
     * @return self
     */
    public function setOidcIssuerUrl(?string $oidcIssuerUrl): self
    {
        $this->initialized['oidcIssuerUrl'] = true;
        $this->oidcIssuerUrl = $oidcIssuerUrl;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getOidcMultiTenant(): ?bool
    {
        return $this->oidcMultiTenant;
    }
    /**
     * @param bool|null $oidcMultiTenant
     *
     * @return self
     */
    public function setOidcMultiTenant(?bool $oidcMultiTenant): self
    {
        $this->initialized['oidcMultiTenant'] = true;
        $this->oidcMultiTenant = $oidcMultiTenant;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getSyncRolesFromIdp(): ?bool
    {
        return $this->syncRolesFromIdp;
    }
    /**
     * @param bool|null $syncRolesFromIdp
     *
     * @return self
     */
    public function setSyncRolesFromIdp(?bool $syncRolesFromIdp): self
    {
        $this->initialized['syncRolesFromIdp'] = true;
        $this->syncRolesFromIdp = $syncRolesFromIdp;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getType(): ?string
    {
        return $this->type;
    }
    /**
     * @param string|null $type
     *
     * @return self
     */
    public function setType(?string $type): self
    {
        $this->initialized['type'] = true;
        $this->type = $type;
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