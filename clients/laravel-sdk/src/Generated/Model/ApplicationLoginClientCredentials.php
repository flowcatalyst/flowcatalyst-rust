<?php

namespace FlowCatalyst\Generated\Model;

class ApplicationLoginClientCredentials
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
    protected $clientType;
    /**
     * @var ApplicationOAuthClientCredentials|null
     */
    protected $oauthClient;
    /**
     * @var list<string>|null
     */
    protected $redirectUris;
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
     * @return ApplicationOAuthClientCredentials|null
     */
    public function getOauthClient(): ?ApplicationOAuthClientCredentials
    {
        return $this->oauthClient;
    }
    /**
     * @param ApplicationOAuthClientCredentials|null $oauthClient
     *
     * @return self
     */
    public function setOauthClient(?ApplicationOAuthClientCredentials $oauthClient): self
    {
        $this->initialized['oauthClient'] = true;
        $this->oauthClient = $oauthClient;
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
}