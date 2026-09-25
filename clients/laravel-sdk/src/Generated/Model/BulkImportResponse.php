<?php

namespace FlowCatalyst\Generated\Model;

class BulkImportResponse
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
    protected $failed;
    /**
     * @var list<BulkImportResult>|null
     */
    protected $results;
    /**
     * @var int|null
     */
    protected $skipped;
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
    public function getFailed(): ?int
    {
        return $this->failed;
    }
    /**
     * @param int|null $failed
     *
     * @return self
     */
    public function setFailed(?int $failed): self
    {
        $this->initialized['failed'] = true;
        $this->failed = $failed;
        return $this;
    }
    /**
     * @return list<BulkImportResult>|null
     */
    public function getResults(): ?array
    {
        return $this->results;
    }
    /**
     * @param list<BulkImportResult>|null $results
     *
     * @return self
     */
    public function setResults(?array $results): self
    {
        $this->initialized['results'] = true;
        $this->results = $results;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getSkipped(): ?int
    {
        return $this->skipped;
    }
    /**
     * @param int|null $skipped
     *
     * @return self
     */
    public function setSkipped(?int $skipped): self
    {
        $this->initialized['skipped'] = true;
        $this->skipped = $skipped;
        return $this;
    }
}