<?php

namespace FlowCatalyst\Generated\Model;

class AssignApplicationAccessRequest extends \ArrayObject
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
    protected $allApplications;
    /**
     * @var list<string>|null
     */
    protected $applicationIds;
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
    public function getAllApplications(): ?bool
    {
        return $this->allApplications;
    }
    /**
     * @param bool|null $allApplications
     *
     * @return self
     */
    public function setAllApplications(?bool $allApplications): self
    {
        $this->initialized['allApplications'] = true;
        $this->allApplications = $allApplications;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getApplicationIds(): ?array
    {
        return $this->applicationIds;
    }
    /**
     * @param list<string>|null $applicationIds
     *
     * @return self
     */
    public function setApplicationIds(?array $applicationIds): self
    {
        $this->initialized['applicationIds'] = true;
        $this->applicationIds = $applicationIds;
        return $this;
    }
}