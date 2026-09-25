<?php

namespace FlowCatalyst\Generated\Model;

class ListOutputBody
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
     * @var list<RequestDTO>|null
     */
    protected $requests;
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
     * @return list<RequestDTO>|null
     */
    public function getRequests(): ?array
    {
        return $this->requests;
    }
    /**
     * @param list<RequestDTO>|null $requests
     *
     * @return self
     */
    public function setRequests(?array $requests): self
    {
        $this->initialized['requests'] = true;
        $this->requests = $requests;
        return $this;
    }
}