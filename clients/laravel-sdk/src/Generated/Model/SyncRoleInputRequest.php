<?php

namespace FlowCatalyst\Generated\Model;

class SyncRoleInputRequest extends \ArrayObject
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
     * @var bool|null
     */
    protected $clientManaged;
    /**
     * @var string|null
     */
    protected $description;
    /**
     * @var string|null
     */
    protected $displayName;
    /**
     * @var string|null
     */
    protected $name;
    /**
     * @var list<string>|null
     */
    protected $permissions;
    /**
     * @return bool|null
     */
    public function getClientManaged(): ?bool
    {
        return $this->clientManaged;
    }
    /**
     * @param bool|null $clientManaged
     *
     * @return self
     */
    public function setClientManaged(?bool $clientManaged): self
    {
        $this->initialized['clientManaged'] = true;
        $this->clientManaged = $clientManaged;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getDescription(): ?string
    {
        return $this->description;
    }
    /**
     * @param string|null $description
     *
     * @return self
     */
    public function setDescription(?string $description): self
    {
        $this->initialized['description'] = true;
        $this->description = $description;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getDisplayName(): ?string
    {
        return $this->displayName;
    }
    /**
     * @param string|null $displayName
     *
     * @return self
     */
    public function setDisplayName(?string $displayName): self
    {
        $this->initialized['displayName'] = true;
        $this->displayName = $displayName;
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
    public function getPermissions(): ?array
    {
        return $this->permissions;
    }
    /**
     * @param list<string>|null $permissions
     *
     * @return self
     */
    public function setPermissions(?array $permissions): self
    {
        $this->initialized['permissions'] = true;
        $this->permissions = $permissions;
        return $this;
    }
}