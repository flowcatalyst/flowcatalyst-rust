<?php

namespace FlowCatalyst\Generated\Model;

class AuthenticateBeginResponse
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
     * @var mixed|null
     */
    protected $options;
    /**
     * @var string|null
     */
    protected $stateId;
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
     * @return mixed
     */
    public function getOptions()
    {
        return $this->options;
    }
    /**
     * @param mixed $options
     *
     * @return self
     */
    public function setOptions($options): self
    {
        $this->initialized['options'] = true;
        $this->options = $options;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getStateId(): ?string
    {
        return $this->stateId;
    }
    /**
     * @param string|null $stateId
     *
     * @return self
     */
    public function setStateId(?string $stateId): self
    {
        $this->initialized['stateId'] = true;
        $this->stateId = $stateId;
        return $this;
    }
}