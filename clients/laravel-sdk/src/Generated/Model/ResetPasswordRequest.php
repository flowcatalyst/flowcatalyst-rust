<?php

namespace FlowCatalyst\Generated\Model;

class ResetPasswordRequest extends \ArrayObject
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
    protected $enforcePasswordComplexity;
    /**
     * @var string|null
     */
    protected $newPassword;
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
    public function getEnforcePasswordComplexity(): ?bool
    {
        return $this->enforcePasswordComplexity;
    }
    /**
     * @param bool|null $enforcePasswordComplexity
     *
     * @return self
     */
    public function setEnforcePasswordComplexity(?bool $enforcePasswordComplexity): self
    {
        $this->initialized['enforcePasswordComplexity'] = true;
        $this->enforcePasswordComplexity = $enforcePasswordComplexity;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getNewPassword(): ?string
    {
        return $this->newPassword;
    }
    /**
     * @param string|null $newPassword
     *
     * @return self
     */
    public function setNewPassword(?string $newPassword): self
    {
        $this->initialized['newPassword'] = true;
        $this->newPassword = $newPassword;
        return $this;
    }
}