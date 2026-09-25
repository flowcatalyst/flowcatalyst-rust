<?php

namespace FlowCatalyst\Generated\Model;

class SyncDocsRequest extends \ArrayObject
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
     * @var list<SyncDocInputRequest>|null
     */
    protected $docs;
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
     * @return list<SyncDocInputRequest>|null
     */
    public function getDocs(): ?array
    {
        return $this->docs;
    }
    /**
     * @param list<SyncDocInputRequest>|null $docs
     *
     * @return self
     */
    public function setDocs(?array $docs): self
    {
        $this->initialized['docs'] = true;
        $this->docs = $docs;
        return $this;
    }
}