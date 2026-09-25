<?php

namespace FlowCatalyst\Generated\Model;

class DispatchJobFilterOptionsResponse
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
     * @var list<string>|null
     */
    protected $clientIds;
    /**
     * @var list<string>|null
     */
    protected $codes;
    /**
     * @var list<string>|null
     */
    protected $dispatchPoolIds;
    /**
     * @var list<string>|null
     */
    protected $kinds;
    /**
     * @var list<string>|null
     */
    protected $statuses;
    /**
     * @var list<string>|null
     */
    protected $subscriptionIds;
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
     * @return list<string>|null
     */
    public function getClientIds(): ?array
    {
        return $this->clientIds;
    }
    /**
     * @param list<string>|null $clientIds
     *
     * @return self
     */
    public function setClientIds(?array $clientIds): self
    {
        $this->initialized['clientIds'] = true;
        $this->clientIds = $clientIds;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getCodes(): ?array
    {
        return $this->codes;
    }
    /**
     * @param list<string>|null $codes
     *
     * @return self
     */
    public function setCodes(?array $codes): self
    {
        $this->initialized['codes'] = true;
        $this->codes = $codes;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getDispatchPoolIds(): ?array
    {
        return $this->dispatchPoolIds;
    }
    /**
     * @param list<string>|null $dispatchPoolIds
     *
     * @return self
     */
    public function setDispatchPoolIds(?array $dispatchPoolIds): self
    {
        $this->initialized['dispatchPoolIds'] = true;
        $this->dispatchPoolIds = $dispatchPoolIds;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getKinds(): ?array
    {
        return $this->kinds;
    }
    /**
     * @param list<string>|null $kinds
     *
     * @return self
     */
    public function setKinds(?array $kinds): self
    {
        $this->initialized['kinds'] = true;
        $this->kinds = $kinds;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getStatuses(): ?array
    {
        return $this->statuses;
    }
    /**
     * @param list<string>|null $statuses
     *
     * @return self
     */
    public function setStatuses(?array $statuses): self
    {
        $this->initialized['statuses'] = true;
        $this->statuses = $statuses;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getSubscriptionIds(): ?array
    {
        return $this->subscriptionIds;
    }
    /**
     * @param list<string>|null $subscriptionIds
     *
     * @return self
     */
    public function setSubscriptionIds(?array $subscriptionIds): self
    {
        $this->initialized['subscriptionIds'] = true;
        $this->subscriptionIds = $subscriptionIds;
        return $this;
    }
}