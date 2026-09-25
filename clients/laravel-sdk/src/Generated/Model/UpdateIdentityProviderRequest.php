<?php

namespace FlowCatalyst\Generated\Model;

class UpdateIdentityProviderRequest extends \ArrayObject
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
     * Desired set of domains routed to this provider; additions are mapped/claimed, removals fall back to internal auth
     *
     * @var list<string>|null
     */
    protected $allowedEmailDomains;
    /**
     * @var list<string>|null
     */
    protected $allowedRoleIds;
    /**
     * Scope for email-domain mappings this request creates: ANCHOR (platform administrators) or CLIENT (requires primaryClientId). Required when the request creates a new mapping. Existing mappings keep their scope; with CLIENT the client is linked on any mapping that has no primary client yet.
     *
     * @var string|null
     */
    protected $mappingScope;
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
     * Client to link on mappings that are new or not yet linked to a primary client
     *
     * @var string|null
     */
    protected $primaryClientId;
    /**
     * @var bool|null
     */
    protected $syncRolesFromIdp;
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
     * Desired set of domains routed to this provider; additions are mapped/claimed, removals fall back to internal auth
     *
     * @return list<string>|null
     */
    public function getAllowedEmailDomains(): ?array
    {
        return $this->allowedEmailDomains;
    }
    /**
     * Desired set of domains routed to this provider; additions are mapped/claimed, removals fall back to internal auth
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
     * Scope for email-domain mappings this request creates: ANCHOR (platform administrators) or CLIENT (requires primaryClientId). Required when the request creates a new mapping. Existing mappings keep their scope; with CLIENT the client is linked on any mapping that has no primary client yet.
     *
     * @return string|null
     */
    public function getMappingScope(): ?string
    {
        return $this->mappingScope;
    }
    /**
     * Scope for email-domain mappings this request creates: ANCHOR (platform administrators) or CLIENT (requires primaryClientId). Required when the request creates a new mapping. Existing mappings keep their scope; with CLIENT the client is linked on any mapping that has no primary client yet.
     *
     * @param string|null $mappingScope
     *
     * @return self
     */
    public function setMappingScope(?string $mappingScope): self
    {
        $this->initialized['mappingScope'] = true;
        $this->mappingScope = $mappingScope;
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
     * Client to link on mappings that are new or not yet linked to a primary client
     *
     * @return string|null
     */
    public function getPrimaryClientId(): ?string
    {
        return $this->primaryClientId;
    }
    /**
     * Client to link on mappings that are new or not yet linked to a primary client
     *
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
}