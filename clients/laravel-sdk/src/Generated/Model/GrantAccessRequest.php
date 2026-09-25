<?php

namespace FlowCatalyst\Generated\Model;

class GrantAccessRequest extends \ArrayObject
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
    protected $canWrite;
    /**
     * @var string|null
     */
    protected $roleCode;
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
    public function getCanWrite(): ?bool
    {
        return $this->canWrite;
    }
    /**
     * @param bool|null $canWrite
     *
     * @return self
     */
    public function setCanWrite(?bool $canWrite): self
    {
        $this->initialized['canWrite'] = true;
        $this->canWrite = $canWrite;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getRoleCode(): ?string
    {
        return $this->roleCode;
    }
    /**
     * @param string|null $roleCode
     *
     * @return self
     */
    public function setRoleCode(?string $roleCode): self
    {
        $this->initialized['roleCode'] = true;
        $this->roleCode = $roleCode;
        return $this;
    }
}