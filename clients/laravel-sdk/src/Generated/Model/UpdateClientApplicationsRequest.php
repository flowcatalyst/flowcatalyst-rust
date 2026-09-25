<?php

namespace FlowCatalyst\Generated\Model;

class UpdateClientApplicationsRequest extends \ArrayObject
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
    protected $enabledApplicationIds;
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
    public function getEnabledApplicationIds(): ?array
    {
        return $this->enabledApplicationIds;
    }
    /**
     * @param list<string>|null $enabledApplicationIds
     *
     * @return self
     */
    public function setEnabledApplicationIds(?array $enabledApplicationIds): self
    {
        $this->initialized['enabledApplicationIds'] = true;
        $this->enabledApplicationIds = $enabledApplicationIds;
        return $this;
    }
}