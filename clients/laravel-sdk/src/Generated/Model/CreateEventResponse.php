<?php

namespace FlowCatalyst\Generated\Model;

class CreateEventResponse
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
     * Number of dispatch jobs created for matching subscriptions
     *
     * @var int|null
     */
    protected $dispatchJobCount;
    /**
     * @var CreatedEvent|null
     */
    protected $event;
    /**
     * True if this was a deduplicated request (event already existed)
     *
     * @var bool|null
     */
    protected $isDuplicate;
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
     * Number of dispatch jobs created for matching subscriptions
     *
     * @return int|null
     */
    public function getDispatchJobCount(): ?int
    {
        return $this->dispatchJobCount;
    }
    /**
     * Number of dispatch jobs created for matching subscriptions
     *
     * @param int|null $dispatchJobCount
     *
     * @return self
     */
    public function setDispatchJobCount(?int $dispatchJobCount): self
    {
        $this->initialized['dispatchJobCount'] = true;
        $this->dispatchJobCount = $dispatchJobCount;
        return $this;
    }
    /**
     * @return CreatedEvent|null
     */
    public function getEvent(): ?CreatedEvent
    {
        return $this->event;
    }
    /**
     * @param CreatedEvent|null $event
     *
     * @return self
     */
    public function setEvent(?CreatedEvent $event): self
    {
        $this->initialized['event'] = true;
        $this->event = $event;
        return $this;
    }
    /**
     * True if this was a deduplicated request (event already existed)
     *
     * @return bool|null
     */
    public function getIsDuplicate(): ?bool
    {
        return $this->isDuplicate;
    }
    /**
     * True if this was a deduplicated request (event already existed)
     *
     * @param bool|null $isDuplicate
     *
     * @return self
     */
    public function setIsDuplicate(?bool $isDuplicate): self
    {
        $this->initialized['isDuplicate'] = true;
        $this->isDuplicate = $isDuplicate;
        return $this;
    }
}