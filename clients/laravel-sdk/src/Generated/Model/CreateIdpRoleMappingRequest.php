<?php

namespace FlowCatalyst\Generated\Model;

class CreateIdpRoleMappingRequest extends \ArrayObject
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
     * @var string|null
     */
    protected $idpRoleName;
    /**
     * @var string|null
     */
    protected $idpType;
    /**
     * @var string|null
     */
    protected $platformRoleName;
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
     * @return string|null
     */
    public function getIdpRoleName(): ?string
    {
        return $this->idpRoleName;
    }
    /**
     * @param string|null $idpRoleName
     *
     * @return self
     */
    public function setIdpRoleName(?string $idpRoleName): self
    {
        $this->initialized['idpRoleName'] = true;
        $this->idpRoleName = $idpRoleName;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getIdpType(): ?string
    {
        return $this->idpType;
    }
    /**
     * @param string|null $idpType
     *
     * @return self
     */
    public function setIdpType(?string $idpType): self
    {
        $this->initialized['idpType'] = true;
        $this->idpType = $idpType;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getPlatformRoleName(): ?string
    {
        return $this->platformRoleName;
    }
    /**
     * @param string|null $platformRoleName
     *
     * @return self
     */
    public function setPlatformRoleName(?string $platformRoleName): self
    {
        $this->initialized['platformRoleName'] = true;
        $this->platformRoleName = $platformRoleName;
        return $this;
    }
}