<?php

namespace FlowCatalyst\Generated\Model;

class ApplicationAccessResponse
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
     * @var string|null
     */
    protected $applicationCode;
    /**
     * @var string|null
     */
    protected $applicationId;
    /**
     * @var string|null
     */
    protected $applicationName;
    /**
     * @return string|null
     */
    public function getApplicationCode(): ?string
    {
        return $this->applicationCode;
    }
    /**
     * @param string|null $applicationCode
     *
     * @return self
     */
    public function setApplicationCode(?string $applicationCode): self
    {
        $this->initialized['applicationCode'] = true;
        $this->applicationCode = $applicationCode;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getApplicationId(): ?string
    {
        return $this->applicationId;
    }
    /**
     * @param string|null $applicationId
     *
     * @return self
     */
    public function setApplicationId(?string $applicationId): self
    {
        $this->initialized['applicationId'] = true;
        $this->applicationId = $applicationId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getApplicationName(): ?string
    {
        return $this->applicationName;
    }
    /**
     * @param string|null $applicationName
     *
     * @return self
     */
    public function setApplicationName(?string $applicationName): self
    {
        $this->initialized['applicationName'] = true;
        $this->applicationName = $applicationName;
        return $this;
    }
}