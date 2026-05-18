<?php

namespace FlowCatalyst\Generated\Model;

class SyncDispatchPoolsRequest extends \ArrayObject
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
     * @var list<SyncDispatchPoolInputRequest>|null
     */
    protected $pools;
    /**
     * @return list<SyncDispatchPoolInputRequest>|null
     */
    public function getPools(): ?array
    {
        return $this->pools;
    }
    /**
     * @param list<SyncDispatchPoolInputRequest>|null $pools
     *
     * @return self
     */
    public function setPools(?array $pools): self
    {
        $this->initialized['pools'] = true;
        $this->pools = $pools;
        return $this;
    }
}