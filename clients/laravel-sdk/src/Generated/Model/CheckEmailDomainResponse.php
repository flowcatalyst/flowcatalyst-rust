<?php

namespace FlowCatalyst\Generated\Model;

class CheckEmailDomainResponse
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
    protected $allowedClientIds;
    /**
     * @var string|null
     */
    protected $authMethod;
    /**
     * @var string|null
     */
    protected $authProvider;
    /**
     * @var string|null
     */
    protected $derivedScope;
    /**
     * @var string|null
     */
    protected $domain;
    /**
     * @var bool|null
     */
    protected $emailExists;
    /**
     * @var bool|null
     */
    protected $hasIdpConfig;
    /**
     * @var string|null
     */
    protected $idpIssuer;
    /**
     * @var string|null
     */
    protected $info;
    /**
     * @var bool|null
     */
    protected $isAnchorDomain;
    /**
     * @var string|null
     */
    protected $loginUrl;
    /**
     * @var bool|null
     */
    protected $requiresClientId;
    /**
     * @var string|null
     */
    protected $warning;
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
    public function getAllowedClientIds(): ?array
    {
        return $this->allowedClientIds;
    }
    /**
     * @param list<string>|null $allowedClientIds
     *
     * @return self
     */
    public function setAllowedClientIds(?array $allowedClientIds): self
    {
        $this->initialized['allowedClientIds'] = true;
        $this->allowedClientIds = $allowedClientIds;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getAuthMethod(): ?string
    {
        return $this->authMethod;
    }
    /**
     * @param string|null $authMethod
     *
     * @return self
     */
    public function setAuthMethod(?string $authMethod): self
    {
        $this->initialized['authMethod'] = true;
        $this->authMethod = $authMethod;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getAuthProvider(): ?string
    {
        return $this->authProvider;
    }
    /**
     * @param string|null $authProvider
     *
     * @return self
     */
    public function setAuthProvider(?string $authProvider): self
    {
        $this->initialized['authProvider'] = true;
        $this->authProvider = $authProvider;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getDerivedScope(): ?string
    {
        return $this->derivedScope;
    }
    /**
     * @param string|null $derivedScope
     *
     * @return self
     */
    public function setDerivedScope(?string $derivedScope): self
    {
        $this->initialized['derivedScope'] = true;
        $this->derivedScope = $derivedScope;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getDomain(): ?string
    {
        return $this->domain;
    }
    /**
     * @param string|null $domain
     *
     * @return self
     */
    public function setDomain(?string $domain): self
    {
        $this->initialized['domain'] = true;
        $this->domain = $domain;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getEmailExists(): ?bool
    {
        return $this->emailExists;
    }
    /**
     * @param bool|null $emailExists
     *
     * @return self
     */
    public function setEmailExists(?bool $emailExists): self
    {
        $this->initialized['emailExists'] = true;
        $this->emailExists = $emailExists;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getHasIdpConfig(): ?bool
    {
        return $this->hasIdpConfig;
    }
    /**
     * @param bool|null $hasIdpConfig
     *
     * @return self
     */
    public function setHasIdpConfig(?bool $hasIdpConfig): self
    {
        $this->initialized['hasIdpConfig'] = true;
        $this->hasIdpConfig = $hasIdpConfig;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getIdpIssuer(): ?string
    {
        return $this->idpIssuer;
    }
    /**
     * @param string|null $idpIssuer
     *
     * @return self
     */
    public function setIdpIssuer(?string $idpIssuer): self
    {
        $this->initialized['idpIssuer'] = true;
        $this->idpIssuer = $idpIssuer;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getInfo(): ?string
    {
        return $this->info;
    }
    /**
     * @param string|null $info
     *
     * @return self
     */
    public function setInfo(?string $info): self
    {
        $this->initialized['info'] = true;
        $this->info = $info;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getIsAnchorDomain(): ?bool
    {
        return $this->isAnchorDomain;
    }
    /**
     * @param bool|null $isAnchorDomain
     *
     * @return self
     */
    public function setIsAnchorDomain(?bool $isAnchorDomain): self
    {
        $this->initialized['isAnchorDomain'] = true;
        $this->isAnchorDomain = $isAnchorDomain;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getLoginUrl(): ?string
    {
        return $this->loginUrl;
    }
    /**
     * @param string|null $loginUrl
     *
     * @return self
     */
    public function setLoginUrl(?string $loginUrl): self
    {
        $this->initialized['loginUrl'] = true;
        $this->loginUrl = $loginUrl;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getRequiresClientId(): ?bool
    {
        return $this->requiresClientId;
    }
    /**
     * @param bool|null $requiresClientId
     *
     * @return self
     */
    public function setRequiresClientId(?bool $requiresClientId): self
    {
        $this->initialized['requiresClientId'] = true;
        $this->requiresClientId = $requiresClientId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getWarning(): ?string
    {
        return $this->warning;
    }
    /**
     * @param string|null $warning
     *
     * @return self
     */
    public function setWarning(?string $warning): self
    {
        $this->initialized['warning'] = true;
        $this->warning = $warning;
        return $this;
    }
}