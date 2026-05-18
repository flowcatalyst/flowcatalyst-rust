<?php

namespace FlowCatalyst\Generated\Model;

class SyncPrincipalsRequest extends \ArrayObject
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
     * @var list<SyncPrincipalInputRequest>|null
     */
    protected $principals;
    /**
     * @return list<SyncPrincipalInputRequest>|null
     */
    public function getPrincipals(): ?array
    {
        return $this->principals;
    }
    /**
     * @param list<SyncPrincipalInputRequest>|null $principals
     *
     * @return self
     */
    public function setPrincipals(?array $principals): self
    {
        $this->initialized['principals'] = true;
        $this->principals = $principals;
        return $this;
    }
}