<?php

namespace FlowCatalyst\Generated\Model;

class CreatePortalAppResponse
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
    protected $clientSecret;
    /**
     * @var string|null
     */
    protected $clientType;
    /**
     * @var string|null
     */
    protected $oauthClientId;
    /**
     * @var string|null
     */
    protected $oauthClientRowId;
    /**
     * @var PortalAppResponse|null
     */
    protected $portalApp;
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
    public function getClientSecret(): ?string
    {
        return $this->clientSecret;
    }
    /**
     * @param string|null $clientSecret
     *
     * @return self
     */
    public function setClientSecret(?string $clientSecret): self
    {
        $this->initialized['clientSecret'] = true;
        $this->clientSecret = $clientSecret;
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
     * @return string|null
     */
    public function getOauthClientId(): ?string
    {
        return $this->oauthClientId;
    }
    /**
     * @param string|null $oauthClientId
     *
     * @return self
     */
    public function setOauthClientId(?string $oauthClientId): self
    {
        $this->initialized['oauthClientId'] = true;
        $this->oauthClientId = $oauthClientId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getOauthClientRowId(): ?string
    {
        return $this->oauthClientRowId;
    }
    /**
     * @param string|null $oauthClientRowId
     *
     * @return self
     */
    public function setOauthClientRowId(?string $oauthClientRowId): self
    {
        $this->initialized['oauthClientRowId'] = true;
        $this->oauthClientRowId = $oauthClientRowId;
        return $this;
    }
    /**
     * @return PortalAppResponse|null
     */
    public function getPortalApp(): ?PortalAppResponse
    {
        return $this->portalApp;
    }
    /**
     * @param PortalAppResponse|null $portalApp
     *
     * @return self
     */
    public function setPortalApp(?PortalAppResponse $portalApp): self
    {
        $this->initialized['portalApp'] = true;
        $this->portalApp = $portalApp;
        return $this;
    }
}