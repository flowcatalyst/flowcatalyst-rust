<?php

namespace FlowCatalyst\Generated\Model;

class PortalUserListItem
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
     * @var list<PortalUserAppRef>|null
     */
    protected $apps;
    /**
     * @var \DateTime|null
     */
    protected $createdAt;
    /**
     * @var string|null
     */
    protected $email;
    /**
     * @var bool|null
     */
    protected $hasPassword;
    /**
     * @var string|null
     */
    protected $identityId;
    /**
     * @var \DateTime|null
     */
    protected $inviteExpiresAt;
    /**
     * @var \DateTime|null
     */
    protected $invitedAt;
    /**
     * @var \DateTime|null
     */
    protected $lastLoginAt;
    /**
     * @var string|null
     */
    protected $name;
    /**
     * @var string|null
     */
    protected $source;
    /**
     * @var string|null
     */
    protected $state;
    /**
     * @var string|null
     */
    protected $status;
    /**
     * @var \DateTime|null
     */
    protected $updatedAt;
    /**
     * @return list<PortalUserAppRef>|null
     */
    public function getApps(): ?array
    {
        return $this->apps;
    }
    /**
     * @param list<PortalUserAppRef>|null $apps
     *
     * @return self
     */
    public function setApps(?array $apps): self
    {
        $this->initialized['apps'] = true;
        $this->apps = $apps;
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
     * @return bool|null
     */
    public function getHasPassword(): ?bool
    {
        return $this->hasPassword;
    }
    /**
     * @param bool|null $hasPassword
     *
     * @return self
     */
    public function setHasPassword(?bool $hasPassword): self
    {
        $this->initialized['hasPassword'] = true;
        $this->hasPassword = $hasPassword;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getIdentityId(): ?string
    {
        return $this->identityId;
    }
    /**
     * @param string|null $identityId
     *
     * @return self
     */
    public function setIdentityId(?string $identityId): self
    {
        $this->initialized['identityId'] = true;
        $this->identityId = $identityId;
        return $this;
    }
    /**
     * @return \DateTime|null
     */
    public function getInviteExpiresAt(): ?\DateTime
    {
        return $this->inviteExpiresAt;
    }
    /**
     * @param \DateTime|null $inviteExpiresAt
     *
     * @return self
     */
    public function setInviteExpiresAt(?\DateTime $inviteExpiresAt): self
    {
        $this->initialized['inviteExpiresAt'] = true;
        $this->inviteExpiresAt = $inviteExpiresAt;
        return $this;
    }
    /**
     * @return \DateTime|null
     */
    public function getInvitedAt(): ?\DateTime
    {
        return $this->invitedAt;
    }
    /**
     * @param \DateTime|null $invitedAt
     *
     * @return self
     */
    public function setInvitedAt(?\DateTime $invitedAt): self
    {
        $this->initialized['invitedAt'] = true;
        $this->invitedAt = $invitedAt;
        return $this;
    }
    /**
     * @return \DateTime|null
     */
    public function getLastLoginAt(): ?\DateTime
    {
        return $this->lastLoginAt;
    }
    /**
     * @param \DateTime|null $lastLoginAt
     *
     * @return self
     */
    public function setLastLoginAt(?\DateTime $lastLoginAt): self
    {
        $this->initialized['lastLoginAt'] = true;
        $this->lastLoginAt = $lastLoginAt;
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
    public function getSource(): ?string
    {
        return $this->source;
    }
    /**
     * @param string|null $source
     *
     * @return self
     */
    public function setSource(?string $source): self
    {
        $this->initialized['source'] = true;
        $this->source = $source;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getState(): ?string
    {
        return $this->state;
    }
    /**
     * @param string|null $state
     *
     * @return self
     */
    public function setState(?string $state): self
    {
        $this->initialized['state'] = true;
        $this->state = $state;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getStatus(): ?string
    {
        return $this->status;
    }
    /**
     * @param string|null $status
     *
     * @return self
     */
    public function setStatus(?string $status): self
    {
        $this->initialized['status'] = true;
        $this->status = $status;
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