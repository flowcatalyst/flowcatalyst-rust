<?php

namespace FlowCatalyst\Generated\Model;

class RolesAssignedResponse
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
    protected $added;
    /**
     * @var list<string>|null
     */
    protected $removed;
    /**
     * @var list<PrincipalRoleAssignmentDTO>|null
     */
    protected $roles;
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
    public function getAdded(): ?array
    {
        return $this->added;
    }
    /**
     * @param list<string>|null $added
     *
     * @return self
     */
    public function setAdded(?array $added): self
    {
        $this->initialized['added'] = true;
        $this->added = $added;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getRemoved(): ?array
    {
        return $this->removed;
    }
    /**
     * @param list<string>|null $removed
     *
     * @return self
     */
    public function setRemoved(?array $removed): self
    {
        $this->initialized['removed'] = true;
        $this->removed = $removed;
        return $this;
    }
    /**
     * @return list<PrincipalRoleAssignmentDTO>|null
     */
    public function getRoles(): ?array
    {
        return $this->roles;
    }
    /**
     * @param list<PrincipalRoleAssignmentDTO>|null $roles
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