<?php

namespace FlowCatalyst\Generated\Model;

class PortalUserListResponse
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
    protected $page;
    /**
     * @var list<PortalUserListItem>|null
     */
    protected $portalUsers;
    /**
     * @var int|null
     */
    protected $size;
    /**
     * @var int|null
     */
    protected $total;
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
    public function getPage(): ?int
    {
        return $this->page;
    }
    /**
     * @param int|null $page
     *
     * @return self
     */
    public function setPage(?int $page): self
    {
        $this->initialized['page'] = true;
        $this->page = $page;
        return $this;
    }
    /**
     * @return list<PortalUserListItem>|null
     */
    public function getPortalUsers(): ?array
    {
        return $this->portalUsers;
    }
    /**
     * @param list<PortalUserListItem>|null $portalUsers
     *
     * @return self
     */
    public function setPortalUsers(?array $portalUsers): self
    {
        $this->initialized['portalUsers'] = true;
        $this->portalUsers = $portalUsers;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getSize(): ?int
    {
        return $this->size;
    }
    /**
     * @param int|null $size
     *
     * @return self
     */
    public function setSize(?int $size): self
    {
        $this->initialized['size'] = true;
        $this->size = $size;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getTotal(): ?int
    {
        return $this->total;
    }
    /**
     * @param int|null $total
     *
     * @return self
     */
    public function setTotal(?int $total): self
    {
        $this->initialized['total'] = true;
        $this->total = $total;
        return $this;
    }
}