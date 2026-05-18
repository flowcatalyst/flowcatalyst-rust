<?php

namespace FlowCatalyst\Generated\Model;

class SyncPrincipalInputRequest extends \ArrayObject
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
     * Whether the user is active (default: true)
     *
     * @var bool|null
     */
    protected $active;
    /**
     * User's email address (unique identifier for matching)
     *
     * @var string|null
     */
    protected $email;
    /**
     * Display name
     *
     * @var string|null
     */
    protected $name;
    /**
     * Role short names to assign (prefixed with applicationCode)
     *
     * @var list<string>|null
     */
    protected $roles;
    /**
     * Whether the user is active (default: true)
     *
     * @return bool|null
     */
    public function getActive(): ?bool
    {
        return $this->active;
    }
    /**
     * Whether the user is active (default: true)
     *
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
     * User's email address (unique identifier for matching)
     *
     * @return string|null
     */
    public function getEmail(): ?string
    {
        return $this->email;
    }
    /**
     * User's email address (unique identifier for matching)
     *
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
     * Display name
     *
     * @return string|null
     */
    public function getName(): ?string
    {
        return $this->name;
    }
    /**
     * Display name
     *
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
     * Role short names to assign (prefixed with applicationCode)
     *
     * @return list<string>|null
     */
    public function getRoles(): ?array
    {
        return $this->roles;
    }
    /**
     * Role short names to assign (prefixed with applicationCode)
     *
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
}