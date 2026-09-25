<?php

namespace FlowCatalyst\Generated\Model;

class AuditLogEntityTypesResponse
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
    protected $entityTypes;
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
    public function getEntityTypes(): ?array
    {
        return $this->entityTypes;
    }
    /**
     * @param list<string>|null $entityTypes
     *
     * @return self
     */
    public function setEntityTypes(?array $entityTypes): self
    {
        $this->initialized['entityTypes'] = true;
        $this->entityTypes = $entityTypes;
        return $this;
    }
}