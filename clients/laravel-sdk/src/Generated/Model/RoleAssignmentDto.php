<?php

namespace FlowCatalyst\Generated\Model;

class RoleAssignmentDTO
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
     * @var \DateTime|null
     */
    protected $assignedAt;
    /**
     * @var string|null
     */
    protected $assignedBy;
    /**
     * @var string|null
     */
    protected $assignmentSource;
    /**
     * @var string|null
     */
    protected $clientId;
    /**
     * @var string|null
     */
    protected $roleName;
    /**
     * @return \DateTime|null
     */
    public function getAssignedAt(): ?\DateTime
    {
        return $this->assignedAt;
    }
    /**
     * @param \DateTime|null $assignedAt
     *
     * @return self
     */
    public function setAssignedAt(?\DateTime $assignedAt): self
    {
        $this->initialized['assignedAt'] = true;
        $this->assignedAt = $assignedAt;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getAssignedBy(): ?string
    {
        return $this->assignedBy;
    }
    /**
     * @param string|null $assignedBy
     *
     * @return self
     */
    public function setAssignedBy(?string $assignedBy): self
    {
        $this->initialized['assignedBy'] = true;
        $this->assignedBy = $assignedBy;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getAssignmentSource(): ?string
    {
        return $this->assignmentSource;
    }
    /**
     * @param string|null $assignmentSource
     *
     * @return self
     */
    public function setAssignmentSource(?string $assignmentSource): self
    {
        $this->initialized['assignmentSource'] = true;
        $this->assignmentSource = $assignmentSource;
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
    public function getRoleName(): ?string
    {
        return $this->roleName;
    }
    /**
     * @param string|null $roleName
     *
     * @return self
     */
    public function setRoleName(?string $roleName): self
    {
        $this->initialized['roleName'] = true;
        $this->roleName = $roleName;
        return $this;
    }
}