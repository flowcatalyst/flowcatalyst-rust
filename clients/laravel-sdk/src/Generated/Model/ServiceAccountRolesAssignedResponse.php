<?php

namespace FlowCatalyst\Generated\Model;

class ServiceAccountRolesAssignedResponse
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
    protected $addedRoles;
    /**
     * @var list<string>|null
     */
    protected $removedRoles;
    /**
     * @var list<RoleAssignmentDTO>|null
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
    public function getAddedRoles(): ?array
    {
        return $this->addedRoles;
    }
    /**
     * @param list<string>|null $addedRoles
     *
     * @return self
     */
    public function setAddedRoles(?array $addedRoles): self
    {
        $this->initialized['addedRoles'] = true;
        $this->addedRoles = $addedRoles;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getRemovedRoles(): ?array
    {
        return $this->removedRoles;
    }
    /**
     * @param list<string>|null $removedRoles
     *
     * @return self
     */
    public function setRemovedRoles(?array $removedRoles): self
    {
        $this->initialized['removedRoles'] = true;
        $this->removedRoles = $removedRoles;
        return $this;
    }
    /**
     * @return list<RoleAssignmentDTO>|null
     */
    public function getRoles(): ?array
    {
        return $this->roles;
    }
    /**
     * @param list<RoleAssignmentDTO>|null $roles
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