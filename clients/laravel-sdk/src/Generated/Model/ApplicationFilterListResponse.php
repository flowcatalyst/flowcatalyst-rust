<?php

namespace FlowCatalyst\Generated\Model;

class ApplicationFilterListResponse
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
    protected $applicationCodes;
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
    public function getApplicationCodes(): ?array
    {
        return $this->applicationCodes;
    }
    /**
     * @param list<string>|null $applicationCodes
     *
     * @return self
     */
    public function setApplicationCodes(?array $applicationCodes): self
    {
        $this->initialized['applicationCodes'] = true;
        $this->applicationCodes = $applicationCodes;
        return $this;
    }
}