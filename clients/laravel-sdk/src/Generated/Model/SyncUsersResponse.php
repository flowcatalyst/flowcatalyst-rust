<?php

namespace FlowCatalyst\Generated\Model;

class SyncUsersResponse
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
    protected $created;
    /**
     * @var int|null
     */
    protected $deleted;
    /**
     * @var list<string>|null
     */
    protected $syncedEmails;
    /**
     * @var int|null
     */
    protected $updated;
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
    public function getSyncedEmails(): ?array
    {
        return $this->syncedEmails;
    }
    /**
     * @param list<string>|null $syncedEmails
     *
     * @return self
     */
    public function setSyncedEmails(?array $syncedEmails): self
    {
        $this->initialized['syncedEmails'] = true;
        $this->syncedEmails = $syncedEmails;
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