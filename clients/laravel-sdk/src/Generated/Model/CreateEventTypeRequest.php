<?php

namespace FlowCatalyst\Generated\Model;

class CreateEventTypeRequest extends \ArrayObject
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
     * Optional client scope; absent means anchor-level
     *
     * @var string|null
     */
    protected $clientId;
    /**
     * Events of this type are per-client
     *
     * @var bool|null
     */
    protected $clientScoped;
    /**
     * Event type code in application:subdomain:aggregate:event format
     *
     * @var string|null
     */
    protected $code;
    /**
     * @var string|null
     */
    protected $description;
    /**
     * Human-readable event type name
     *
     * @var string|null
     */
    protected $name;
    /**
     * Optional JSON Schema for the initial spec version
     *
     * @var mixed|null
     */
    protected $schema;
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
     * Optional client scope; absent means anchor-level
     *
     * @return string|null
     */
    public function getClientId(): ?string
    {
        return $this->clientId;
    }
    /**
     * Optional client scope; absent means anchor-level
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
     * Events of this type are per-client
     *
     * @return bool|null
     */
    public function getClientScoped(): ?bool
    {
        return $this->clientScoped;
    }
    /**
     * Events of this type are per-client
     *
     * @param bool|null $clientScoped
     *
     * @return self
     */
    public function setClientScoped(?bool $clientScoped): self
    {
        $this->initialized['clientScoped'] = true;
        $this->clientScoped = $clientScoped;
        return $this;
    }
    /**
     * Event type code in application:subdomain:aggregate:event format
     *
     * @return string|null
     */
    public function getCode(): ?string
    {
        return $this->code;
    }
    /**
     * Event type code in application:subdomain:aggregate:event format
     *
     * @param string|null $code
     *
     * @return self
     */
    public function setCode(?string $code): self
    {
        $this->initialized['code'] = true;
        $this->code = $code;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getDescription(): ?string
    {
        return $this->description;
    }
    /**
     * @param string|null $description
     *
     * @return self
     */
    public function setDescription(?string $description): self
    {
        $this->initialized['description'] = true;
        $this->description = $description;
        return $this;
    }
    /**
     * Human-readable event type name
     *
     * @return string|null
     */
    public function getName(): ?string
    {
        return $this->name;
    }
    /**
     * Human-readable event type name
     *
     * @param string|null $name
     *
     * @return self
     */
    public function setName(?string $name): self
    {
        $this->initialized['name'] = true;
        $this->name = $name;
        return $this;
    }
    /**
     * Optional JSON Schema for the initial spec version
     *
     * @return mixed
     */
    public function getSchema()
    {
        return $this->schema;
    }
    /**
     * Optional JSON Schema for the initial spec version
     *
     * @param mixed $schema
     *
     * @return self
     */
    public function setSchema($schema): self
    {
        $this->initialized['schema'] = true;
        $this->schema = $schema;
        return $this;
    }
}