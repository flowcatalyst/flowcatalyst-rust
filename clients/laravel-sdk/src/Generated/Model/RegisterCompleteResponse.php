<?php

namespace FlowCatalyst\Generated\Model;

class RegisterCompleteResponse
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
    protected $credentialId;
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
    public function getCredentialId(): ?string
    {
        return $this->credentialId;
    }
    /**
     * @param string|null $credentialId
     *
     * @return self
     */
    public function setCredentialId(?string $credentialId): self
    {
        $this->initialized['credentialId'] = true;
        $this->credentialId = $credentialId;
        return $this;
    }
}