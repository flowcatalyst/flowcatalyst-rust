<?php

namespace FlowCatalyst\Generated\Model;

class WriteInstanceLogRequest extends \ArrayObject
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
     * DEBUG | INFO | WARN | ERROR
     *
     * @var string|null
     */
    protected $level;
    /**
     * @var string|null
     */
    protected $message;
    /**
     * @var mixed|null
     */
    protected $metadata;
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
     * DEBUG | INFO | WARN | ERROR
     *
     * @return string|null
     */
    public function getLevel(): ?string
    {
        return $this->level;
    }
    /**
     * DEBUG | INFO | WARN | ERROR
     *
     * @param string|null $level
     *
     * @return self
     */
    public function setLevel(?string $level): self
    {
        $this->initialized['level'] = true;
        $this->level = $level;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getMessage(): ?string
    {
        return $this->message;
    }
    /**
     * @param string|null $message
     *
     * @return self
     */
    public function setMessage(?string $message): self
    {
        $this->initialized['message'] = true;
        $this->message = $message;
        return $this;
    }
    /**
     * @return mixed
     */
    public function getMetadata()
    {
        return $this->metadata;
    }
    /**
     * @param mixed $metadata
     *
     * @return self
     */
    public function setMetadata($metadata): self
    {
        $this->initialized['metadata'] = true;
        $this->metadata = $metadata;
        return $this;
    }
}