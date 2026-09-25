<?php

namespace FlowCatalyst\Generated\Model;

class PrincipalResponse
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
     * @var bool|null
     */
    protected $active;
    /**
     * @var string|null
     */
    protected $clientId;
    /**
     * @var \DateTime|null
     */
    protected $createdAt;
    /**
     * @var \DateTime|null
     */
    protected $developerCredentialUpdatedAt;
    /**
     * @var string|null
     */
    protected $email;
    /**
     * @var list<string>|null
     */
    protected $grantedClientIds;
    /**
     * @var bool|null
     */
    protected $hasDeveloperCredential;
    /**
     * @var string|null
     */
    protected $id;
    /**
     * @var string|null
     */
    protected $idpType;
    /**
     * @var string|null
     */
    protected $inviteLink;
    /**
     * @var bool|null
     */
    protected $isAnchorUser;
    /**
     * @var string|null
     */
    protected $name;
    /**
     * @var list<string>|null
     */
    protected $roles;
    /**
     * @var string|null
     */
    protected $scope;
    /**
     * @var list<string>|null
     */
    protected $twoFactorMethods;
    /**
     * @var string|null
     */
    protected $type;
    /**
     * @var \DateTime|null
     */
    protected $updatedAt;
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
     * @return bool|null
     */
    public function getActive(): ?bool
    {
        return $this->active;
    }
    /**
     * @param bool|null $active
     *
     * @return self
     */
    public function setActive(?bool $active): self
    {
        $this->initialized['active'] = true;
        $this->active = $active;
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
     * @return \DateTime|null
     */
    public function getCreatedAt(): ?\DateTime
    {
        return $this->createdAt;
    }
    /**
     * @param \DateTime|null $createdAt
     *
     * @return self
     */
    public function setCreatedAt(?\DateTime $createdAt): self
    {
        $this->initialized['createdAt'] = true;
        $this->createdAt = $createdAt;
        return $this;
    }
    /**
     * @return \DateTime|null
     */
    public function getDeveloperCredentialUpdatedAt(): ?\DateTime
    {
        return $this->developerCredentialUpdatedAt;
    }
    /**
     * @param \DateTime|null $developerCredentialUpdatedAt
     *
     * @return self
     */
    public function setDeveloperCredentialUpdatedAt(?\DateTime $developerCredentialUpdatedAt): self
    {
        $this->initialized['developerCredentialUpdatedAt'] = true;
        $this->developerCredentialUpdatedAt = $developerCredentialUpdatedAt;
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
     * @return list<string>|null
     */
    public function getGrantedClientIds(): ?array
    {
        return $this->grantedClientIds;
    }
    /**
     * @param list<string>|null $grantedClientIds
     *
     * @return self
     */
    public function setGrantedClientIds(?array $grantedClientIds): self
    {
        $this->initialized['grantedClientIds'] = true;
        $this->grantedClientIds = $grantedClientIds;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getHasDeveloperCredential(): ?bool
    {
        return $this->hasDeveloperCredential;
    }
    /**
     * @param bool|null $hasDeveloperCredential
     *
     * @return self
     */
    public function setHasDeveloperCredential(?bool $hasDeveloperCredential): self
    {
        $this->initialized['hasDeveloperCredential'] = true;
        $this->hasDeveloperCredential = $hasDeveloperCredential;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getId(): ?string
    {
        return $this->id;
    }
    /**
     * @param string|null $id
     *
     * @return self
     */
    public function setId(?string $id): self
    {
        $this->initialized['id'] = true;
        $this->id = $id;
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
    public function getInviteLink(): ?string
    {
        return $this->inviteLink;
    }
    /**
     * @param string|null $inviteLink
     *
     * @return self
     */
    public function setInviteLink(?string $inviteLink): self
    {
        $this->initialized['inviteLink'] = true;
        $this->inviteLink = $inviteLink;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getIsAnchorUser(): ?bool
    {
        return $this->isAnchorUser;
    }
    /**
     * @param bool|null $isAnchorUser
     *
     * @return self
     */
    public function setIsAnchorUser(?bool $isAnchorUser): self
    {
        $this->initialized['isAnchorUser'] = true;
        $this->isAnchorUser = $isAnchorUser;
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
     * @return list<string>|null
     */
    public function getRoles(): ?array
    {
        return $this->roles;
    }
    /**
     * @param list<string>|null $roles
     *
     * @return self
     */
    public function setRoles(?array $roles): self
    {
        $this->initialized['roles'] = true;
        $this->roles = $roles;
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
     * @return list<string>|null
     */
    public function getTwoFactorMethods(): ?array
    {
        return $this->twoFactorMethods;
    }
    /**
     * @param list<string>|null $twoFactorMethods
     *
     * @return self
     */
    public function setTwoFactorMethods(?array $twoFactorMethods): self
    {
        $this->initialized['twoFactorMethods'] = true;
        $this->twoFactorMethods = $twoFactorMethods;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getType(): ?string
    {
        return $this->type;
    }
    /**
     * @param string|null $type
     *
     * @return self
     */
    public function setType(?string $type): self
    {
        $this->initialized['type'] = true;
        $this->type = $type;
        return $this;
    }
    /**
     * @return \DateTime|null
     */
    public function getUpdatedAt(): ?\DateTime
    {
        return $this->updatedAt;
    }
    /**
     * @param \DateTime|null $updatedAt
     *
     * @return self
     */
    public function setUpdatedAt(?\DateTime $updatedAt): self
    {
        $this->initialized['updatedAt'] = true;
        $this->updatedAt = $updatedAt;
        return $this;
    }
}