<?php

namespace FlowCatalyst\Generated\Model;

class SendPasswordResetInputBody extends \ArrayObject
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
     * @var bool|null
     */
    protected $reset2fa;
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
     * @return bool|null
     */
    public function getReset2fa(): ?bool
    {
        return $this->reset2fa;
    }
    /**
     * @param bool|null $reset2fa
     *
     * @return self
     */
    public function setReset2fa(?bool $reset2fa): self
    {
        $this->initialized['reset2fa'] = true;
        $this->reset2fa = $reset2fa;
        return $this;
    }
}