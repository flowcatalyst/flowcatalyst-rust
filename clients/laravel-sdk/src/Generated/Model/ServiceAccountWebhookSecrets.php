<?php

namespace FlowCatalyst\Generated\Model;

class ServiceAccountWebhookSecrets
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
    protected $authToken;
    /**
     * @var string|null
     */
    protected $signingSecret;
    /**
     * @return string|null
     */
    public function getAuthToken(): ?string
    {
        return $this->authToken;
    }
    /**
     * @param string|null $authToken
     *
     * @return self
     */
    public function setAuthToken(?string $authToken): self
    {
        $this->initialized['authToken'] = true;
        $this->authToken = $authToken;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getSigningSecret(): ?string
    {
        return $this->signingSecret;
    }
    /**
     * @param string|null $signingSecret
     *
     * @return self
     */
    public function setSigningSecret(?string $signingSecret): self
    {
        $this->initialized['signingSecret'] = true;
        $this->signingSecret = $signingSecret;
        return $this;
    }
}