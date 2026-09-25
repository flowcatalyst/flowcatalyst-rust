<?php

namespace FlowCatalyst\Generated\Model;

class ServiceAccountTokenResponse
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
    protected $accessToken;
    /**
     * @var int|null
     */
    protected $expiresIn;
    /**
     * @var string|null
     */
    protected $scope;
    /**
     * @var string|null
     */
    protected $tokenType;
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
    public function getAccessToken(): ?string
    {
        return $this->accessToken;
    }
    /**
     * @param string|null $accessToken
     *
     * @return self
     */
    public function setAccessToken(?string $accessToken): self
    {
        $this->initialized['accessToken'] = true;
        $this->accessToken = $accessToken;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getExpiresIn(): ?int
    {
        return $this->expiresIn;
    }
    /**
     * @param int|null $expiresIn
     *
     * @return self
     */
    public function setExpiresIn(?int $expiresIn): self
    {
        $this->initialized['expiresIn'] = true;
        $this->expiresIn = $expiresIn;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getScope(): ?string
    {
        return $this->scope;
    }
    /**
     * @param string|null $scope
     *
     * @return self
     */
    public function setScope(?string $scope): self
    {
        $this->initialized['scope'] = true;
        $this->scope = $scope;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getTokenType(): ?string
    {
        return $this->tokenType;
    }
    /**
     * @param string|null $tokenType
     *
     * @return self
     */
    public function setTokenType(?string $tokenType): self
    {
        $this->initialized['tokenType'] = true;
        $this->tokenType = $tokenType;
        return $this;
    }
}