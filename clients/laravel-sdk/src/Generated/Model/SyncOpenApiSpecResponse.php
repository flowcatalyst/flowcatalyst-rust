<?php

namespace FlowCatalyst\Generated\Model;

class SyncOpenApiSpecResponse extends \ArrayObject
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
    protected $archivedPriorVersion;
    /**
     * @var bool|null
     */
    protected $hasBreaking;
    /**
     * @var string|null
     */
    protected $specId;
    /**
     * @var string|null
     */
    protected $status;
    /**
     * @var bool|null
     */
    protected $unchanged;
    /**
     * @var string|null
     */
    protected $version;
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
    public function getArchivedPriorVersion(): ?string
    {
        return $this->archivedPriorVersion;
    }
    /**
     * @param string|null $archivedPriorVersion
     *
     * @return self
     */
    public function setArchivedPriorVersion(?string $archivedPriorVersion): self
    {
        $this->initialized['archivedPriorVersion'] = true;
        $this->archivedPriorVersion = $archivedPriorVersion;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getHasBreaking(): ?bool
    {
        return $this->hasBreaking;
    }
    /**
     * @param bool|null $hasBreaking
     *
     * @return self
     */
    public function setHasBreaking(?bool $hasBreaking): self
    {
        $this->initialized['hasBreaking'] = true;
        $this->hasBreaking = $hasBreaking;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getSpecId(): ?string
    {
        return $this->specId;
    }
    /**
     * @param string|null $specId
     *
     * @return self
     */
    public function setSpecId(?string $specId): self
    {
        $this->initialized['specId'] = true;
        $this->specId = $specId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getStatus(): ?string
    {
        return $this->status;
    }
    /**
     * @param string|null $status
     *
     * @return self
     */
    public function setStatus(?string $status): self
    {
        $this->initialized['status'] = true;
        $this->status = $status;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getUnchanged(): ?bool
    {
        return $this->unchanged;
    }
    /**
     * @param bool|null $unchanged
     *
     * @return self
     */
    public function setUnchanged(?bool $unchanged): self
    {
        $this->initialized['unchanged'] = true;
        $this->unchanged = $unchanged;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getVersion(): ?string
    {
        return $this->version;
    }
    /**
     * @param string|null $version
     *
     * @return self
     */
    public function setVersion(?string $version): self
    {
        $this->initialized['version'] = true;
        $this->version = $version;
        return $this;
    }
}