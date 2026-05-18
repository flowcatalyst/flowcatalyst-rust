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
     * @var list<SyncProcessInputRequest>|null
     */
    protected $processes;
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