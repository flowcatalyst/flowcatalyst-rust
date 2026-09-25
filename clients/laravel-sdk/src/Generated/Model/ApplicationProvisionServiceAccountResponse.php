<?php

namespace FlowCatalyst\Generated\Model;

class ApplicationProvisionServiceAccountResponse
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
    protected $message;
    /**
     * @var ApplicationServiceAccountCredentials|null
     */
    protected $serviceAccount;
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
     * @return ApplicationServiceAccountCredentials|null
     */
    public function getServiceAccount(): ?ApplicationServiceAccountCredentials
    {
        return $this->serviceAccount;
    }
    /**
     * @param ApplicationServiceAccountCredentials|null $serviceAccount
     *
     * @return self
     */
    public function setServiceAccount(?ApplicationServiceAccountCredentials $serviceAccount): self
    {
        $this->initialized['serviceAccount'] = true;
        $this->serviceAccount = $serviceAccount;
        return $this;
    }
}