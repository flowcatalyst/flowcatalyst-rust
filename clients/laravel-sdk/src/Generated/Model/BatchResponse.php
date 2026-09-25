<?php

namespace FlowCatalyst\Generated\Model;

class BatchResponse
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
     * @var list<BatchResultItem>|null
     */
    protected $results;
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
     * @return list<BatchResultItem>|null
     */
    public function getResults(): ?array
    {
        return $this->results;
    }
    /**
     * @param list<BatchResultItem>|null $results
     *
     * @return self
     */
    public function setResults(?array $results): self
    {
        $this->initialized['results'] = true;
        $this->results = $results;
        return $this;
    }
}