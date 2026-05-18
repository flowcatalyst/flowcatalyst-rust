<?php

namespace FlowCatalyst\Generated\Model;

class SyncEventTypesRequest extends \ArrayObject
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
     * @var list<SyncEventTypeInputRequest>|null
     */
    protected $eventTypes;
    /**
     * @return list<SyncEventTypeInputRequest>|null
     */
    public function getEventTypes(): ?array
    {
        return $this->eventTypes;
    }
    /**
     * @param list<SyncEventTypeInputRequest>|null $eventTypes
     *
     * @return self
     */
    public function setEventTypes(?array $eventTypes): self
    {
        $this->initialized['eventTypes'] = true;
        $this->eventTypes = $eventTypes;
        return $this;
    }
}