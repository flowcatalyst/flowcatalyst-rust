<?php

namespace FlowCatalyst\Generated\Model;

class SyncProcessInputRequest extends \ArrayObject
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
     * Diagram body (typically Mermaid source).
     *
     * @var string|null
     */
    protected $body;
    /**
     * Full code (application:subdomain:process-name)
     *
     * @var string|null
     */
    protected $code;
    /**
     * @var string|null
     */
    protected $description;
    /**
     * @var string|null
     */
    protected $diagramType;
    /**
     * @var string|null
     */
    protected $name;
    /**
     * @var list<string>|null
     */
    protected $tags;
    /**
     * Diagram body (typically Mermaid source).
     *
     * @return string|null
     */
    public function getBody(): ?string
    {
        return $this->body;
    }
    /**
     * Diagram body (typically Mermaid source).
     *
     * @param string|null $body
     *
     * @return self
     */
    public function setBody(?string $body): self
    {
        $this->initialized['body'] = true;
        $this->body = $body;
        return $this;
    }
    /**
     * Full code (application:subdomain:process-name)
     *
     * @return string|null
     */
    public function getCode(): ?string
    {
        return $this->code;
    }
    /**
     * Full code (application:subdomain:process-name)
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
     * @return string|null
     */
    public function getDiagramType(): ?string
    {
        return $this->diagramType;
    }
    /**
     * @param string|null $diagramType
     *
     * @return self
     */
    public function setDiagramType(?string $diagramType): self
    {
        $this->initialized['diagramType'] = true;
        $this->diagramType = $diagramType;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getName(): ?string
    {
        return $this->name;
    }
    /**
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
     * @return list<string>|null
     */
    public function getTags(): ?array
    {
        return $this->tags;
    }
    /**
     * @param list<string>|null $tags
     *
     * @return self
     */
    public function setTags(?array $tags): self
    {
        $this->initialized['tags'] = true;
        $this->tags = $tags;
        return $this;
    }
}