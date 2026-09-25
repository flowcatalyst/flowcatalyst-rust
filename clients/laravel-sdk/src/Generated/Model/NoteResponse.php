<?php

namespace FlowCatalyst\Generated\Model;

class NoteResponse
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
     * @var \DateTime|null
     */
    protected $addedAt;
    /**
     * @var string|null
     */
    protected $addedBy;
    /**
     * @var string|null
     */
    protected $category;
    /**
     * @var string|null
     */
    protected $text;
    /**
     * @return \DateTime|null
     */
    public function getAddedAt(): ?\DateTime
    {
        return $this->addedAt;
    }
    /**
     * @param \DateTime|null $addedAt
     *
     * @return self
     */
    public function setAddedAt(?\DateTime $addedAt): self
    {
        $this->initialized['addedAt'] = true;
        $this->addedAt = $addedAt;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getAddedBy(): ?string
    {
        return $this->addedBy;
    }
    /**
     * @param string|null $addedBy
     *
     * @return self
     */
    public function setAddedBy(?string $addedBy): self
    {
        $this->initialized['addedBy'] = true;
        $this->addedBy = $addedBy;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getCategory(): ?string
    {
        return $this->category;
    }
    /**
     * @param string|null $category
     *
     * @return self
     */
    public function setCategory(?string $category): self
    {
        $this->initialized['category'] = true;
        $this->category = $category;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getText(): ?string
    {
        return $this->text;
    }
    /**
     * @param string|null $text
     *
     * @return self
     */
    public function setText(?string $text): self
    {
        $this->initialized['text'] = true;
        $this->text = $text;
        return $this;
    }
}