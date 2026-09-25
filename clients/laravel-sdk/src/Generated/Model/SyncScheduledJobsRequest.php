<?php

namespace FlowCatalyst\Generated\Model;

class SyncScheduledJobsRequest extends \ArrayObject
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
    protected $archiveUnlisted;
    /**
     * @var string|null
     */
    protected $clientId;
    /**
     * @var list<SyncScheduledJobInputRequest>|null
     */
    protected $jobs;
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
    public function getArchiveUnlisted(): ?bool
    {
        return $this->archiveUnlisted;
    }
    /**
     * @param bool|null $archiveUnlisted
     *
     * @return self
     */
    public function setArchiveUnlisted(?bool $archiveUnlisted): self
    {
        $this->initialized['archiveUnlisted'] = true;
        $this->archiveUnlisted = $archiveUnlisted;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getClientId(): ?string
    {
        return $this->clientId;
    }
    /**
     * @param string|null $clientId
     *
     * @return self
     */
    public function setClientId(?string $clientId): self
    {
        $this->initialized['clientId'] = true;
        $this->clientId = $clientId;
        return $this;
    }
    /**
     * @return list<SyncScheduledJobInputRequest>|null
     */
    public function getJobs(): ?array
    {
        return $this->jobs;
    }
    /**
     * @param list<SyncScheduledJobInputRequest>|null $jobs
     *
     * @return self
     */
    public function setJobs(?array $jobs): self
    {
        $this->initialized['jobs'] = true;
        $this->jobs = $jobs;
        return $this;
    }
}