<?php

namespace FlowCatalyst\Generated\Model;

class BulkImportResult
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
     * @var string|null
     */
    protected $email;
    /**
     * @var string|null
     */
    protected $message;
    /**
     * @var int|null
     */
    protected $row;
    /**
     * created | exists | dropped | error
     *
     * @var string|null
     */
    protected $status;
    /**
     * @return string|null
     */
    public function getEmail(): ?string
    {
        return $this->email;
    }
    /**
     * @param string|null $email
     *
     * @return self
     */
    public function setEmail(?string $email): self
    {
        $this->initialized['email'] = true;
        $this->email = $email;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getMessage(): ?string
    {
        return $this->message;
    }
    /**
     * @param string|null $message
     *
     * @return self
     */
    public function setMessage(?string $message): self
    {
        $this->initialized['message'] = true;
        $this->message = $message;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getRow(): ?int
    {
        return $this->row;
    }
    /**
     * @param int|null $row
     *
     * @return self
     */
    public function setRow(?int $row): self
    {
        $this->initialized['row'] = true;
        $this->row = $row;
        return $this;
    }
    /**
     * created | exists | dropped | error
     *
     * @return string|null
     */
    public function getStatus(): ?string
    {
        return $this->status;
    }
    /**
     * created | exists | dropped | error
     *
     * @param string|null $status
     *
     * @return self
     */
    public function setStatus(?string $status): self
    {
        $this->initialized['status'] = true;
        $this->status = $status;
        return $this;
    }
}