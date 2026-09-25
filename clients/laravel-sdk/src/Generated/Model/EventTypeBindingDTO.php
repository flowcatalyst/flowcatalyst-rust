<?php

namespace FlowCatalyst\Generated\Model;

class EventTypeBindingDTO
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
    protected $eventTypeId;
    /**
     * @var string|null
     */
    protected $filter;
    /**
     * @var string|null
     */
    protected $specVersion;
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
    public function getEventTypeId(): ?string
    {
        return $this->eventTypeId;
    }
    /**
     * @param string|null $eventTypeId
     *
     * @return self
     */
    public function setEventTypeId(?string $eventTypeId): self
    {
        $this->initialized['eventTypeId'] = true;
        $this->eventTypeId = $eventTypeId;
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
    /**
     * @return string|null
     */
    public function getSpecVersion(): ?string
    {
        return $this->specVersion;
    }
    /**
     * @param string|null $specVersion
     *
     * @return self
     */
    public function setSpecVersion(?string $specVersion): self
    {
        $this->initialized['specVersion'] = true;
        $this->specVersion = $specVersion;
        return $this;
    }
}