<?php

namespace FlowCatalyst\Generated\Model;

class CreateServiceAccountResponse
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
     * @var ServiceAccountOAuthSecrets|null
     */
    protected $oauth;
    /**
     * @var string|null
     */
    protected $principalId;
    /**
     * @var ServiceAccountResponse|null
     */
    protected $serviceAccount;
    /**
     * @var ServiceAccountWebhookSecrets|null
     */
    protected $webhook;
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
     * @return ServiceAccountOAuthSecrets|null
     */
    public function getOauth(): ?ServiceAccountOAuthSecrets
    {
        return $this->oauth;
    }
    /**
     * @param ServiceAccountOAuthSecrets|null $oauth
     *
     * @return self
     */
    public function setOauth(?ServiceAccountOAuthSecrets $oauth): self
    {
        $this->initialized['oauth'] = true;
        $this->oauth = $oauth;
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
    /**
     * @return ServiceAccountResponse|null
     */
    public function getServiceAccount(): ?ServiceAccountResponse
    {
        return $this->serviceAccount;
    }
    /**
     * @param ServiceAccountResponse|null $serviceAccount
     *
     * @return self
     */
    public function setServiceAccount(?ServiceAccountResponse $serviceAccount): self
    {
        $this->initialized['serviceAccount'] = true;
        $this->serviceAccount = $serviceAccount;
        return $this;
    }
    /**
     * @return ServiceAccountWebhookSecrets|null
     */
    public function getWebhook(): ?ServiceAccountWebhookSecrets
    {
        return $this->webhook;
    }
    /**
     * @param ServiceAccountWebhookSecrets|null $webhook
     *
     * @return self
     */
    public function setWebhook(?ServiceAccountWebhookSecrets $webhook): self
    {
        $this->initialized['webhook'] = true;
        $this->webhook = $webhook;
        return $this;
    }
}