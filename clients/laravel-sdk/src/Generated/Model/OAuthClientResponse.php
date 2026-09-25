<?php

namespace FlowCatalyst\Generated\Model;

class OAuthClientResponse
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
     * @var bool|null
     */
    protected $active;
    /**
     * @var list<string>|null
     */
    protected $allowedOrigins;
    /**
     * @var bool|null
     */
    protected $apiAccess;
    /**
     * @var list<string>|null
     */
    protected $applicationIds;
    /**
     * @var list<OAuthClientApplicationRef>|null
     */
    protected $applications;
    /**
     * @var string|null
     */
    protected $clientId;
    /**
     * @var string|null
     */
    protected $clientName;
    /**
     * @var string|null
     */
    protected $clientType;
    /**
     * @var \DateTime|null
     */
    protected $createdAt;
    /**
     * @var list<string>|null
     */
    protected $defaultScopes;
    /**
     * @var list<string>|null
     */
    protected $grantTypes;
    /**
     * @var string|null
     */
    protected $id;
    /**
     * @var bool|null
     */
    protected $pkceRequired;
    /**
     * @var string|null
     */
    protected $portalAppId;
    /**
     * @var string|null
     */
    protected $portalClientId;
    /**
     * @var list<string>|null
     */
    protected $postLogoutRedirectUris;
    /**
     * @var \DateTime|null
     */
    protected $previousSecretExpiresAt;
    /**
     * @var \DateTime|null
     */
    protected $previousSecretLastUsedAt;
    /**
     * @var list<string>|null
     */
    protected $redirectUris;
    /**
     * @var string|null
     */
    protected $serviceAccountPrincipalId;
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
     * @return bool|null
     */
    public function getActive(): ?bool
    {
        return $this->active;
    }
    /**
     * @param bool|null $active
     *
     * @return self
     */
    public function setActive(?bool $active): self
    {
        $this->initialized['active'] = true;
        $this->active = $active;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getAllowedOrigins(): ?array
    {
        return $this->allowedOrigins;
    }
    /**
     * @param list<string>|null $allowedOrigins
     *
     * @return self
     */
    public function setAllowedOrigins(?array $allowedOrigins): self
    {
        $this->initialized['allowedOrigins'] = true;
        $this->allowedOrigins = $allowedOrigins;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getApiAccess(): ?bool
    {
        return $this->apiAccess;
    }
    /**
     * @param bool|null $apiAccess
     *
     * @return self
     */
    public function setApiAccess(?bool $apiAccess): self
    {
        $this->initialized['apiAccess'] = true;
        $this->apiAccess = $apiAccess;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getApplicationIds(): ?array
    {
        return $this->applicationIds;
    }
    /**
     * @param list<string>|null $applicationIds
     *
     * @return self
     */
    public function setApplicationIds(?array $applicationIds): self
    {
        $this->initialized['applicationIds'] = true;
        $this->applicationIds = $applicationIds;
        return $this;
    }
    /**
     * @return list<OAuthClientApplicationRef>|null
     */
    public function getApplications(): ?array
    {
        return $this->applications;
    }
    /**
     * @param list<OAuthClientApplicationRef>|null $applications
     *
     * @return self
     */
    public function setApplications(?array $applications): self
    {
        $this->initialized['applications'] = true;
        $this->applications = $applications;
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
    public function getClientName(): ?string
    {
        return $this->clientName;
    }
    /**
     * @param string|null $clientName
     *
     * @return self
     */
    public function setClientName(?string $clientName): self
    {
        $this->initialized['clientName'] = true;
        $this->clientName = $clientName;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getClientType(): ?string
    {
        return $this->clientType;
    }
    /**
     * @param string|null $clientType
     *
     * @return self
     */
    public function setClientType(?string $clientType): self
    {
        $this->initialized['clientType'] = true;
        $this->clientType = $clientType;
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
     * @return list<string>|null
     */
    public function getDefaultScopes(): ?array
    {
        return $this->defaultScopes;
    }
    /**
     * @param list<string>|null $defaultScopes
     *
     * @return self
     */
    public function setDefaultScopes(?array $defaultScopes): self
    {
        $this->initialized['defaultScopes'] = true;
        $this->defaultScopes = $defaultScopes;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getGrantTypes(): ?array
    {
        return $this->grantTypes;
    }
    /**
     * @param list<string>|null $grantTypes
     *
     * @return self
     */
    public function setGrantTypes(?array $grantTypes): self
    {
        $this->initialized['grantTypes'] = true;
        $this->grantTypes = $grantTypes;
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
     * @return bool|null
     */
    public function getPkceRequired(): ?bool
    {
        return $this->pkceRequired;
    }
    /**
     * @param bool|null $pkceRequired
     *
     * @return self
     */
    public function setPkceRequired(?bool $pkceRequired): self
    {
        $this->initialized['pkceRequired'] = true;
        $this->pkceRequired = $pkceRequired;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getPortalAppId(): ?string
    {
        return $this->portalAppId;
    }
    /**
     * @param string|null $portalAppId
     *
     * @return self
     */
    public function setPortalAppId(?string $portalAppId): self
    {
        $this->initialized['portalAppId'] = true;
        $this->portalAppId = $portalAppId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getPortalClientId(): ?string
    {
        return $this->portalClientId;
    }
    /**
     * @param string|null $portalClientId
     *
     * @return self
     */
    public function setPortalClientId(?string $portalClientId): self
    {
        $this->initialized['portalClientId'] = true;
        $this->portalClientId = $portalClientId;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getPostLogoutRedirectUris(): ?array
    {
        return $this->postLogoutRedirectUris;
    }
    /**
     * @param list<string>|null $postLogoutRedirectUris
     *
     * @return self
     */
    public function setPostLogoutRedirectUris(?array $postLogoutRedirectUris): self
    {
        $this->initialized['postLogoutRedirectUris'] = true;
        $this->postLogoutRedirectUris = $postLogoutRedirectUris;
        return $this;
    }
    /**
     * @return \DateTime|null
     */
    public function getPreviousSecretExpiresAt(): ?\DateTime
    {
        return $this->previousSecretExpiresAt;
    }
    /**
     * @param \DateTime|null $previousSecretExpiresAt
     *
     * @return self
     */
    public function setPreviousSecretExpiresAt(?\DateTime $previousSecretExpiresAt): self
    {
        $this->initialized['previousSecretExpiresAt'] = true;
        $this->previousSecretExpiresAt = $previousSecretExpiresAt;
        return $this;
    }
    /**
     * @return \DateTime|null
     */
    public function getPreviousSecretLastUsedAt(): ?\DateTime
    {
        return $this->previousSecretLastUsedAt;
    }
    /**
     * @param \DateTime|null $previousSecretLastUsedAt
     *
     * @return self
     */
    public function setPreviousSecretLastUsedAt(?\DateTime $previousSecretLastUsedAt): self
    {
        $this->initialized['previousSecretLastUsedAt'] = true;
        $this->previousSecretLastUsedAt = $previousSecretLastUsedAt;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getRedirectUris(): ?array
    {
        return $this->redirectUris;
    }
    /**
     * @param list<string>|null $redirectUris
     *
     * @return self
     */
    public function setRedirectUris(?array $redirectUris): self
    {
        $this->initialized['redirectUris'] = true;
        $this->redirectUris = $redirectUris;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getServiceAccountPrincipalId(): ?string
    {
        return $this->serviceAccountPrincipalId;
    }
    /**
     * @param string|null $serviceAccountPrincipalId
     *
     * @return self
     */
    public function setServiceAccountPrincipalId(?string $serviceAccountPrincipalId): self
    {
        $this->initialized['serviceAccountPrincipalId'] = true;
        $this->serviceAccountPrincipalId = $serviceAccountPrincipalId;
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