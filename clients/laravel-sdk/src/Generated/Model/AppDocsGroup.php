<?php

namespace FlowCatalyst\Generated\Model;

class AppDocsGroup
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
    protected $applicationName;
    /**
     * @var list<DocSummary>|null
     */
    protected $docs;
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
    /**
     * @return list<DocSummary>|null
     */
    public function getDocs(): ?array
    {
        return $this->docs;
    }
    /**
     * @param list<DocSummary>|null $docs
     *
     * @return self
     */
    public function setDocs(?array $docs): self
    {
        $this->initialized['docs'] = true;
        $this->docs = $docs;
        return $this;
    }
}