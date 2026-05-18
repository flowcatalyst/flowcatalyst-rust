<?php

namespace FlowCatalyst\Generated\Model;

class SyncScheduledJobsResultResponse extends \ArrayObject
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
     * @var list<string>|null
     */
    protected $archived;
    /**
     * @var list<string>|null
     */
    protected $created;
    /**
     * @var list<string>|null
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
     * @return list<string>|null
     */
    public function getArchived(): ?array
    {
        return $this->archived;
    }
    /**
     * @param list<string>|null $archived
     *
     * @return self
     */
    public function setArchived(?array $archived): self
    {
        $this->initialized['archived'] = true;
        $this->archived = $archived;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getCreated(): ?array
    {
        return $this->created;
    }
    /**
     * @param list<string>|null $created
     *
     * @return self
     */
    public function setCreated(?array $created): self
    {
        $this->initialized['created'] = true;
        $this->created = $created;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getUpdated(): ?array
    {
        return $this->updated;
    }
    /**
     * @param list<string>|null $updated
     *
     * @return self
     */
    public function setUpdated(?array $updated): self
    {
        $this->initialized['updated'] = true;
        $this->updated = $updated;
        return $this;
    }
}