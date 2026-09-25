<?php

namespace FlowCatalyst\Generated\Model;

class SetApplicationAccessResponse
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
    protected $added;
    /**
     * @var bool|null
     */
    protected $allApplications;
    /**
     * @var list<ApplicationAccessResponse>|null
     */
    protected $applications;
    /**
     * @var int|null
     */
    protected $removed;
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
    public function getAdded(): ?int
    {
        return $this->added;
    }
    /**
     * @param int|null $added
     *
     * @return self
     */
    public function setAdded(?int $added): self
    {
        $this->initialized['added'] = true;
        $this->added = $added;
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
     * @return list<ApplicationAccessResponse>|null
     */
    public function getApplications(): ?array
    {
        return $this->applications;
    }
    /**
     * @param list<ApplicationAccessResponse>|null $applications
     *
     * @return self
     */
    public function setApplications(?array $applications): self
    {
        $this->initialized['applications'] = true;
        $this->applications = $applications;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getRemoved(): ?int
    {
        return $this->removed;
    }
    /**
     * @param int|null $removed
     *
     * @return self
     */
    public function setRemoved(?int $removed): self
    {
        $this->initialized['removed'] = true;
        $this->removed = $removed;
        return $this;
    }
}