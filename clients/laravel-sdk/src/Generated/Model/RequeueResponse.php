<?php

namespace FlowCatalyst\Generated\Model;

class RequeueResponse
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
    protected $requeued;
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
    public function getRequeued(): ?int
    {
        return $this->requeued;
    }
    /**
     * @param int|null $requeued
     *
     * @return self
     */
    public function setRequeued(?int $requeued): self
    {
        $this->initialized['requeued'] = true;
        $this->requeued = $requeued;
        return $this;
    }
}