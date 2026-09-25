<?php

namespace FlowCatalyst\Generated\Model;

class SyncDocInputRequest
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
     * The page body, Markdown (Mermaid fences render as diagrams)
     *
     * @var string|null
     */
    protected $content;
    /**
     * URL-safe page id, unique within the application (kebab-case)
     *
     * @var string|null
     */
    protected $slug;
    /**
     * @var string|null
     */
    protected $title;
    /**
     * The page body, Markdown (Mermaid fences render as diagrams)
     *
     * @return string|null
     */
    public function getContent(): ?string
    {
        return $this->content;
    }
    /**
     * The page body, Markdown (Mermaid fences render as diagrams)
     *
     * @param string|null $content
     *
     * @return self
     */
    public function setContent(?string $content): self
    {
        $this->initialized['content'] = true;
        $this->content = $content;
        return $this;
    }
    /**
     * URL-safe page id, unique within the application (kebab-case)
     *
     * @return string|null
     */
    public function getSlug(): ?string
    {
        return $this->slug;
    }
    /**
     * URL-safe page id, unique within the application (kebab-case)
     *
     * @param string|null $slug
     *
     * @return self
     */
    public function setSlug(?string $slug): self
    {
        $this->initialized['slug'] = true;
        $this->slug = $slug;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getTitle(): ?string
    {
        return $this->title;
    }
    /**
     * @param string|null $title
     *
     * @return self
     */
    public function setTitle(?string $title): self
    {
        $this->initialized['title'] = true;
        $this->title = $title;
        return $this;
    }
}