<?php

namespace FlowCatalyst\Generated\Model;

class SyncOpenApiSpecRequest extends \ArrayObject
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
     * The OpenAPI document (OpenAPI 3.x or Swagger 2.x).
     *
     * @var mixed|null
     */
    protected $spec;
    /**
     * The OpenAPI document (OpenAPI 3.x or Swagger 2.x).
     *
     * @return mixed
     */
    public function getSpec()
    {
        return $this->spec;
    }
    /**
     * The OpenAPI document (OpenAPI 3.x or Swagger 2.x).
     *
     * @param mixed $spec
     *
     * @return self
     */
    public function setSpec($spec): self
    {
        $this->initialized['spec'] = true;
        $this->spec = $spec;
        return $this;
    }
}