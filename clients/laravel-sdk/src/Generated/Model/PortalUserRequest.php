<?php

namespace FlowCatalyst\Generated\Model;

class PortalUserRequest extends \ArrayObject
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
    protected $clientId;
    /**
     * @var string|null
     */
    protected $email;
    /**
     * @var string|null
     */
    protected $name;
    /**
     * @var string|null
     */
    protected $portalAppCode;
    /**
     * @var string|null
     */
    protected $redirectUri;
    /**
     * @var bool|null
     */
    protected $returnInviteLink;
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
    public function getClientId(): ?string
    {
        return $this->clientId;
    }
    /**
     * @param string|null $clientId
     *
     * @return self
     */
    public function setClientId(?string $clientId): self
    {
        $this->initialized['clientId'] = true;
        $this->clientId = $clientId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getEmail(): ?string
    {
        return $this->email;
    }
    /**
     * @param string|null $email
     *
     * @return self
     */
    public function setEmail(?string $email): self
    {
        $this->initialized['email'] = true;
        $this->email = $email;
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
    public function getPortalAppCode(): ?string
    {
        return $this->portalAppCode;
    }
    /**
     * @param string|null $portalAppCode
     *
     * @return self
     */
    public function setPortalAppCode(?string $portalAppCode): self
    {
        $this->initialized['portalAppCode'] = true;
        $this->portalAppCode = $portalAppCode;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getRedirectUri(): ?string
    {
        return $this->redirectUri;
    }
    /**
     * @param string|null $redirectUri
     *
     * @return self
     */
    public function setRedirectUri(?string $redirectUri): self
    {
        $this->initialized['redirectUri'] = true;
        $this->redirectUri = $redirectUri;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getReturnInviteLink(): ?bool
    {
        return $this->returnInviteLink;
    }
    /**
     * @param bool|null $returnInviteLink
     *
     * @return self
     */
    public function setReturnInviteLink(?bool $returnInviteLink): self
    {
        $this->initialized['returnInviteLink'] = true;
        $this->returnInviteLink = $returnInviteLink;
        return $this;
    }
}