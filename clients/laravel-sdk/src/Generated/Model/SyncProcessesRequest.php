<?php

namespace FlowCatalyst\Generated\Model;

class SyncProcessesRequest extends \ArrayObject
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
     * @var list<SyncProcessInputRequest>|null
     */
    protected $processes;
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
     * @return list<SyncProcessInputRequest>|null
     */
    public function getProcesses(): ?array
    {
        return $this->processes;
    }
    /**
     * @param list<SyncProcessInputRequest>|null $processes
     *
     * @return self
     */
    public function setProcesses(?array $processes): self
    {
        $this->initialized['processes'] = true;
        $this->processes = $processes;
        return $this;
    }
}