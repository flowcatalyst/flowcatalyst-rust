<?php

namespace FlowCatalyst\Generated\Model;

class UpdateAuthConfigRequest extends \ArrayObject
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
     * @var string|null
     */
    protected $authProvider;
    /**
     * @var list<string>|null
     */
    protected $grantedClientIds;
    /**
     * @var string|null
     */
    protected $oidcClientId;
    /**
     * @var string|null
     */
    protected $oidcClientSecretRef;
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
     * @var string|null
     */
    protected $primaryClientId;
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
    public function getOidcClientSecretRef(): ?string
    {
        return $this->oidcClientSecretRef;
    }
    /**
     * @param string|null $oidcClientSecretRef
     *
     * @return self
     */
    public function setOidcClientSecretRef(?string $oidcClientSecretRef): self
    {
        $this->initialized['oidcClientSecretRef'] = true;
        $this->oidcClientSecretRef = $oidcClientSecretRef;
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
}