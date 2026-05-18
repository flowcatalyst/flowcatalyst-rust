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
     * @var bool|null
     */
    protected $archiveUnlisted;
    /**
     * None = sync platform-scoped jobs (anchor only).
     *
     * @var string|null
     */
    protected $clientId;
    /**
     * @var list<SyncScheduledJobInputRequest>|null
     */
    protected $jobs;
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
     * None = sync platform-scoped jobs (anchor only).
     *
     * @return string|null
     */
    public function getClientId(): ?string
    {
        return $this->clientId;
    }
    /**
     * None = sync platform-scoped jobs (anchor only).
     *
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