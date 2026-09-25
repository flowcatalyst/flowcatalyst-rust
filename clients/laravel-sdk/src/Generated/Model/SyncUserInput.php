<?php

namespace FlowCatalyst\Generated\Model;

class SyncUserInput
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
     * Whether the user is active (default true)
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
     * Pre-hashed password (bcrypt/argon2i/argon2id), stored verbatim; migrated on first login. Omit to leave any existing password untouched.
     *
     * @var string|null
     */
    protected $passwordHash;
    /**
     * Role names to assign (SDK_SYNC source; replaces this source's prior set)
     *
     * @var list<string>|null
     */
    protected $roles;
    /**
     * Whether the user is active (default true)
     *
     * @return bool|null
     */
    public function getActive(): ?bool
    {
        return $this->active;
    }
    /**
     * Whether the user is active (default true)
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
     * Pre-hashed password (bcrypt/argon2i/argon2id), stored verbatim; migrated on first login. Omit to leave any existing password untouched.
     *
     * @return string|null
     */
    public function getPasswordHash(): ?string
    {
        return $this->passwordHash;
    }
    /**
     * Pre-hashed password (bcrypt/argon2i/argon2id), stored verbatim; migrated on first login. Omit to leave any existing password untouched.
     *
     * @param string|null $passwordHash
     *
     * @return self
     */
    public function setPasswordHash(?string $passwordHash): self
    {
        $this->initialized['passwordHash'] = true;
        $this->passwordHash = $passwordHash;
        return $this;
    }
    /**
     * Role names to assign (SDK_SYNC source; replaces this source's prior set)
     *
     * @return list<string>|null
     */
    public function getRoles(): ?array
    {
        return $this->roles;
    }
    /**
     * Role names to assign (SDK_SYNC source; replaces this source's prior set)
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