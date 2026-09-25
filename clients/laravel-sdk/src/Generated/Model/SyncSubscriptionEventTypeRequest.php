<?php

namespace FlowCatalyst\Generated\Model;

class SyncSubscriptionEventTypeRequest
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
     * @var string|null
     */
    protected $eventTypeCode;
    /**
     * @var string|null
     */
    protected $filter;
    /**
     * @return string|null
     */
    public function getEventTypeCode(): ?string
    {
        return $this->eventTypeCode;
    }
    /**
     * @param string|null $eventTypeCode
     *
     * @return self
     */
    public function setEventTypeCode(?string $eventTypeCode): self
    {
        $this->initialized['eventTypeCode'] = true;
        $this->eventTypeCode = $eventTypeCode;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getFilter(): ?string
    {
        return $this->filter;
    }
    /**
     * @param string|null $filter
     *
     * @return self
     */
    public function setFilter(?string $filter): self
    {
        $this->initialized['filter'] = true;
        $this->filter = $filter;
        return $this;
    }
}