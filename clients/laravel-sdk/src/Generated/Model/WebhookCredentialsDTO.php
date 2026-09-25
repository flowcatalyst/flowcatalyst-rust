<?php

namespace FlowCatalyst\Generated\Model;

class WebhookCredentialsDTO
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
    protected $authType;
    /**
     * @var string|null
     */
    protected $headerName;
    /**
     * @var string|null
     */
    protected $password;
    /**
     * @var string|null
     */
    protected $signatureHeader;
    /**
     * @var string|null
     */
    protected $signingAlgorithm;
    /**
     * @var string|null
     */
    protected $signingSecret;
    /**
     * @var string|null
     */
    protected $token;
    /**
     * @var string|null
     */
    protected $username;
    /**
     * @return string|null
     */
    public function getAuthType(): ?string
    {
        return $this->authType;
    }
    /**
     * @param string|null $authType
     *
     * @return self
     */
    public function setAuthType(?string $authType): self
    {
        $this->initialized['authType'] = true;
        $this->authType = $authType;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getHeaderName(): ?string
    {
        return $this->headerName;
    }
    /**
     * @param string|null $headerName
     *
     * @return self
     */
    public function setHeaderName(?string $headerName): self
    {
        $this->initialized['headerName'] = true;
        $this->headerName = $headerName;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getPassword(): ?string
    {
        return $this->password;
    }
    /**
     * @param string|null $password
     *
     * @return self
     */
    public function setPassword(?string $password): self
    {
        $this->initialized['password'] = true;
        $this->password = $password;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getSignatureHeader(): ?string
    {
        return $this->signatureHeader;
    }
    /**
     * @param string|null $signatureHeader
     *
     * @return self
     */
    public function setSignatureHeader(?string $signatureHeader): self
    {
        $this->initialized['signatureHeader'] = true;
        $this->signatureHeader = $signatureHeader;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getSigningAlgorithm(): ?string
    {
        return $this->signingAlgorithm;
    }
    /**
     * @param string|null $signingAlgorithm
     *
     * @return self
     */
    public function setSigningAlgorithm(?string $signingAlgorithm): self
    {
        $this->initialized['signingAlgorithm'] = true;
        $this->signingAlgorithm = $signingAlgorithm;
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
    /**
     * @return string|null
     */
    public function getToken(): ?string
    {
        return $this->token;
    }
    /**
     * @param string|null $token
     *
     * @return self
     */
    public function setToken(?string $token): self
    {
        $this->initialized['token'] = true;
        $this->token = $token;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getUsername(): ?string
    {
        return $this->username;
    }
    /**
     * @param string|null $username
     *
     * @return self
     */
    public function setUsername(?string $username): self
    {
        $this->initialized['username'] = true;
        $this->username = $username;
        return $this;
    }
}