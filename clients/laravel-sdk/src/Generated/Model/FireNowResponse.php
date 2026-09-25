<?php

namespace FlowCatalyst\Generated\Model;

class FireNowResponse
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
    protected $id;
    /**
     * @var string|null
     */
    protected $instanceId;
    /**
     * @var string|null
     */
    protected $scheduledJobId;
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
    public function getId(): ?string
    {
        return $this->id;
    }
    /**
     * @param string|null $id
     *
     * @return self
     */
    public function setId(?string $id): self
    {
        $this->initialized['id'] = true;
        $this->id = $id;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getInstanceId(): ?string
    {
        return $this->instanceId;
    }
    /**
     * @param string|null $instanceId
     *
     * @return self
     */
    public function setInstanceId(?string $instanceId): self
    {
        $this->initialized['instanceId'] = true;
        $this->instanceId = $instanceId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getScheduledJobId(): ?string
    {
        return $this->scheduledJobId;
    }
    /**
     * @param string|null $scheduledJobId
     *
     * @return self
     */
    public function setScheduledJobId(?string $scheduledJobId): self
    {
        $this->initialized['scheduledJobId'] = true;
        $this->scheduledJobId = $scheduledJobId;
        return $this;
    }
}