<?php

namespace FlowCatalyst\Generated\Model;

class AssignUnassignedResponse
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
     * @var int|null
     */
    protected $assigned;
    /**
     * @var string|null
     */
    protected $portalAppCode;
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
     * @return int|null
     */
    public function getAssigned(): ?int
    {
        return $this->assigned;
    }
    /**
     * @param int|null $assigned
     *
     * @return self
     */
    public function setAssigned(?int $assigned): self
    {
        $this->initialized['assigned'] = true;
        $this->assigned = $assigned;
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
}