<?php

namespace FlowCatalyst\Generated\Model;

class CreateEventRequest extends \ArrayObject
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
     * Causation ID - the event that caused this event
     *
     * @var string|null
     */
    protected $causationId;
    /**
     * Client ID (optional, defaults to caller's client)
     *
     * @var string|null
     */
    protected $clientId;
    /**
     * Context data for filtering/searching
     *
     * @var list<ContextEntryDTO>|null
     */
    protected $contextData;
    /**
     * Correlation ID for request tracing
     *
     * @var string|null
     */
    protected $correlationId;
    /**
     * Event payload data
     *
     * @var mixed|null
     */
    protected $data;
    /**
     * Deduplication ID for exactly-once delivery
     *
     * @var string|null
     */
    protected $deduplicationId;
    /**
     * Event type code (e.g., "orders:fulfillment:shipment:shipped")
     *
     * @var string|null
     */
    protected $eventType;
    /**
     * Message group for FIFO ordering
     *
     * @var string|null
     */
    protected $messageGroup;
    /**
     * Event source URI
     *
     * @var string|null
     */
    protected $source;
    /**
     * Event subject (optional context)
     *
     * @var string|null
     */
    protected $subject;
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
     * Causation ID - the event that caused this event
     *
     * @return string|null
     */
    public function getCausationId(): ?string
    {
        return $this->causationId;
    }
    /**
     * Causation ID - the event that caused this event
     *
     * @param string|null $causationId
     *
     * @return self
     */
    public function setCausationId(?string $causationId): self
    {
        $this->initialized['causationId'] = true;
        $this->causationId = $causationId;
        return $this;
    }
    /**
     * Client ID (optional, defaults to caller's client)
     *
     * @return string|null
     */
    public function getClientId(): ?string
    {
        return $this->clientId;
    }
    /**
     * Client ID (optional, defaults to caller's client)
     *
     * @param string|null $clientId
     *
     * @return self
     */
    public function setClientId(?string $clientId): self
    {
        $this->initialized['clientId'] = true;
        $this->clientId = $clientId;
        return $this;
    }
    /**
     * Context data for filtering/searching
     *
     * @return list<ContextEntryDTO>|null
     */
    public function getContextData(): ?array
    {
        return $this->contextData;
    }
    /**
     * Context data for filtering/searching
     *
     * @param list<ContextEntryDTO>|null $contextData
     *
     * @return self
     */
    public function setContextData(?array $contextData): self
    {
        $this->initialized['contextData'] = true;
        $this->contextData = $contextData;
        return $this;
    }
    /**
     * Correlation ID for request tracing
     *
     * @return string|null
     */
    public function getCorrelationId(): ?string
    {
        return $this->correlationId;
    }
    /**
     * Correlation ID for request tracing
     *
     * @param string|null $correlationId
     *
     * @return self
     */
    public function setCorrelationId(?string $correlationId): self
    {
        $this->initialized['correlationId'] = true;
        $this->correlationId = $correlationId;
        return $this;
    }
    /**
     * Event payload data
     *
     * @return mixed
     */
    public function getData()
    {
        return $this->data;
    }
    /**
     * Event payload data
     *
     * @param mixed $data
     *
     * @return self
     */
    public function setData($data): self
    {
        $this->initialized['data'] = true;
        $this->data = $data;
        return $this;
    }
    /**
     * Deduplication ID for exactly-once delivery
     *
     * @return string|null
     */
    public function getDeduplicationId(): ?string
    {
        return $this->deduplicationId;
    }
    /**
     * Deduplication ID for exactly-once delivery
     *
     * @param string|null $deduplicationId
     *
     * @return self
     */
    public function setDeduplicationId(?string $deduplicationId): self
    {
        $this->initialized['deduplicationId'] = true;
        $this->deduplicationId = $deduplicationId;
        return $this;
    }
    /**
     * Event type code (e.g., "orders:fulfillment:shipment:shipped")
     *
     * @return string|null
     */
    public function getEventType(): ?string
    {
        return $this->eventType;
    }
    /**
     * Event type code (e.g., "orders:fulfillment:shipment:shipped")
     *
     * @param string|null $eventType
     *
     * @return self
     */
    public function setEventType(?string $eventType): self
    {
        $this->initialized['eventType'] = true;
        $this->eventType = $eventType;
        return $this;
    }
    /**
     * Message group for FIFO ordering
     *
     * @return string|null
     */
    public function getMessageGroup(): ?string
    {
        return $this->messageGroup;
    }
    /**
     * Message group for FIFO ordering
     *
     * @param string|null $messageGroup
     *
     * @return self
     */
    public function setMessageGroup(?string $messageGroup): self
    {
        $this->initialized['messageGroup'] = true;
        $this->messageGroup = $messageGroup;
        return $this;
    }
    /**
     * Event source URI
     *
     * @return string|null
     */
    public function getSource(): ?string
    {
        return $this->source;
    }
    /**
     * Event source URI
     *
     * @param string|null $source
     *
     * @return self
     */
    public function setSource(?string $source): self
    {
        $this->initialized['source'] = true;
        $this->source = $source;
        return $this;
    }
    /**
     * Event subject (optional context)
     *
     * @return string|null
     */
    public function getSubject(): ?string
    {
        return $this->subject;
    }
    /**
     * Event subject (optional context)
     *
     * @param string|null $subject
     *
     * @return self
     */
    public function setSubject(?string $subject): self
    {
        $this->initialized['subject'] = true;
        $this->subject = $subject;
        return $this;
    }
}