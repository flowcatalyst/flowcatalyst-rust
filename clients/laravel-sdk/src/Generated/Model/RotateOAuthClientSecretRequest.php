<?php

namespace FlowCatalyst\Generated\Model;

class RotateOAuthClientSecretRequest extends \ArrayObject
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
     * @var int|null
     */
    protected $graceSeconds;
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
     * @return int|null
     */
    public function getGraceSeconds(): ?int
    {
        return $this->graceSeconds;
    }
    /**
     * @param int|null $graceSeconds
     *
     * @return self
     */
    public function setGraceSeconds(?int $graceSeconds): self
    {
        $this->initialized['graceSeconds'] = true;
        $this->graceSeconds = $graceSeconds;
        return $this;
    }
}