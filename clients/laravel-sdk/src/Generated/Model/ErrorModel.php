<?php

namespace FlowCatalyst\Generated\Model;

class ErrorModel
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
     * @var array<string, mixed>|null
     */
    protected $details;
    /**
     * @var string|null
     */
    protected $error;
    /**
     * @var string|null
     */
    protected $message;
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
     * @return array<string, mixed>|null
     */
    public function getDetails(): ?iterable
    {
        return $this->details;
    }
    /**
     * @param array<string, mixed>|null $details
     *
     * @return self
     */
    public function setDetails(?iterable $details): self
    {
        $this->initialized['details'] = true;
        $this->details = $details;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getError(): ?string
    {
        return $this->error;
    }
    /**
     * @param string|null $error
     *
     * @return self
     */
    public function setError(?string $error): self
    {
        $this->initialized['error'] = true;
        $this->error = $error;
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
}