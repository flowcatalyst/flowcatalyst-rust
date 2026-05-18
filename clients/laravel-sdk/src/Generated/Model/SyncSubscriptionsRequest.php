<?php

namespace FlowCatalyst\Generated\Model;

class SyncSubscriptionsRequest extends \ArrayObject
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
     * @var list<SyncSubscriptionInputRequest>|null
     */
    protected $subscriptions;
    /**
     * @return list<SyncSubscriptionInputRequest>|null
     */
    public function getSubscriptions(): ?array
    {
        return $this->subscriptions;
    }
    /**
     * @param list<SyncSubscriptionInputRequest>|null $subscriptions
     *
     * @return self
     */
    public function setSubscriptions(?array $subscriptions): self
    {
        $this->initialized['subscriptions'] = true;
        $this->subscriptions = $subscriptions;
        return $this;
    }
}