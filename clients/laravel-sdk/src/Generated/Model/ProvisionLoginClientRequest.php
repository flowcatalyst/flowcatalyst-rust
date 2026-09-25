<?php

namespace FlowCatalyst\Generated\Model;

class ProvisionLoginClientRequest extends \ArrayObject
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
    protected $allowedOrigins;
    /**
     * PUBLIC (default) or CONFIDENTIAL
     *
     * @var string|null
     */
    protected $clientType;
    /**
     * @var list<string>|null
     */
    protected $redirectUris;
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
     * PUBLIC (default) or CONFIDENTIAL
     *
     * @return string|null
     */
    public function getClientType(): ?string
    {
        return $this->clientType;
    }
    /**
     * PUBLIC (default) or CONFIDENTIAL
     *
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