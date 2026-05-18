<?php

namespace FlowCatalyst\Generated\Model;

class SyncResultResponse extends \ArrayObject
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
     * @var int|null
     */
    protected $created;
    /**
     * @var int|null
     */
    protected $deleted;
    /**
     * @var list<string>|null
     */
    protected $syncedCodes;
    /**
     * @var int|null
     */
    protected $updated;
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
     * @return int|null
     */
    public function getCreated(): ?int
    {
        return $this->created;
    }
    /**
     * @param int|null $created
     *
     * @return self
     */
    public function setCreated(?int $created): self
    {
        $this->initialized['created'] = true;
        $this->created = $created;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getDeleted(): ?int
    {
        return $this->deleted;
    }
    /**
     * @param int|null $deleted
     *
     * @return self
     */
    public function setDeleted(?int $deleted): self
    {
        $this->initialized['deleted'] = true;
        $this->deleted = $deleted;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getSyncedCodes(): ?array
    {
        return $this->syncedCodes;
    }
    /**
     * @param list<string>|null $syncedCodes
     *
     * @return self
     */
    public function setSyncedCodes(?array $syncedCodes): self
    {
        $this->initialized['syncedCodes'] = true;
        $this->syncedCodes = $syncedCodes;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getUpdated(): ?int
    {
        return $this->updated;
    }
    /**
     * @param int|null $updated
     *
     * @return self
     */
    public function setUpdated(?int $updated): self
    {
        $this->initialized['updated'] = true;
        $this->updated = $updated;
        return $this;
    }
}