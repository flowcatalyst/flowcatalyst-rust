<?php

namespace FlowCatalyst\Generated\Model;

class CreatePrincipalRequest extends \ArrayObject
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
    protected $idpType;
    /**
     * @var string|null
     */
    protected $inviteRedirectUri;
    /**
     * @var string|null
     */
    protected $name;
    /**
     * @var string|null
     */
    protected $password;
    /**
     * @var bool|null
     */
    protected $returnInviteLink;
    /**
     * Principal scope (ANCHOR, PARTNER, CLIENT)
     *
     * @var string|null
     */
    protected $scope;
    /**
     * @var bool|null
     */
    protected $sendInvitation;
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
    public function getIdpType(): ?string
    {
        return $this->idpType;
    }
    /**
     * @param string|null $idpType
     *
     * @return self
     */
    public function setIdpType(?string $idpType): self
    {
        $this->initialized['idpType'] = true;
        $this->idpType = $idpType;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getInviteRedirectUri(): ?string
    {
        return $this->inviteRedirectUri;
    }
    /**
     * @param string|null $inviteRedirectUri
     *
     * @return self
     */
    public function setInviteRedirectUri(?string $inviteRedirectUri): self
    {
        $this->initialized['inviteRedirectUri'] = true;
        $this->inviteRedirectUri = $inviteRedirectUri;
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
    /**
     * Principal scope (ANCHOR, PARTNER, CLIENT)
     *
     * @return string|null
     */
    public function getScope(): ?string
    {
        return $this->scope;
    }
    /**
     * Principal scope (ANCHOR, PARTNER, CLIENT)
     *
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
     * @return bool|null
     */
    public function getSendInvitation(): ?bool
    {
        return $this->sendInvitation;
    }
    /**
     * @param bool|null $sendInvitation
     *
     * @return self
     */
    public function setSendInvitation(?bool $sendInvitation): self
    {
        $this->initialized['sendInvitation'] = true;
        $this->sendInvitation = $sendInvitation;
        return $this;
    }
}