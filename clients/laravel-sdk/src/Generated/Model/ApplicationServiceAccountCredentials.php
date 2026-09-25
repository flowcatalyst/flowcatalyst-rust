<?php

namespace FlowCatalyst\Generated\Model;

class ApplicationServiceAccountCredentials
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
    protected $name;
    /**
     * @var ApplicationOAuthClientCredentials|null
     */
    protected $oauthClient;
    /**
     * @var string|null
     */
    protected $principalId;
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
     * @return string|null
     */
    public function getPrincipalId(): ?string
    {
        return $this->principalId;
    }
    /**
     * @param string|null $principalId
     *
     * @return self
     */
    public function setPrincipalId(?string $principalId): self
    {
        $this->initialized['principalId'] = true;
        $this->principalId = $principalId;
        return $this;
    }
}