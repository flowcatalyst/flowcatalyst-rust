<?php

namespace FlowCatalyst\Generated\Model;

class SyncProcessesByBodyRequest extends \ArrayObject
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
     * Application code (carried in the body for /api/processes/sync)
     *
     * @var string|null
     */
    protected $applicationCode;
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
     * Application code (carried in the body for /api/processes/sync)
     *
     * @return string|null
     */
    public function getApplicationCode(): ?string
    {
        return $this->applicationCode;
    }
    /**
     * Application code (carried in the body for /api/processes/sync)
     *
     * @param string|null $applicationCode
     *
     * @return self
     */
    public function setApplicationCode(?string $applicationCode): self
    {
        $this->initialized['applicationCode'] = true;
        $this->applicationCode = $applicationCode;
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