<?php

namespace FlowCatalyst\Generated\Model;

class CompleteInstanceRequest extends \ArrayObject
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
     * @var mixed|null
     */
    protected $completionResult;
    /**
     * @var string|null
     */
    protected $completionStatus;
    /**
     * SDK alias for completionResult
     *
     * @var mixed|null
     */
    protected $result;
    /**
     * COMPLETED|FAILED|… (instance status) or SUCCESS|FAILURE (completion outcome)
     *
     * @var string|null
     */
    protected $status;
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
     * @return mixed
     */
    public function getCompletionResult()
    {
        return $this->completionResult;
    }
    /**
     * @param mixed $completionResult
     *
     * @return self
     */
    public function setCompletionResult($completionResult): self
    {
        $this->initialized['completionResult'] = true;
        $this->completionResult = $completionResult;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getCompletionStatus(): ?string
    {
        return $this->completionStatus;
    }
    /**
     * @param string|null $completionStatus
     *
     * @return self
     */
    public function setCompletionStatus(?string $completionStatus): self
    {
        $this->initialized['completionStatus'] = true;
        $this->completionStatus = $completionStatus;
        return $this;
    }
    /**
     * SDK alias for completionResult
     *
     * @return mixed
     */
    public function getResult()
    {
        return $this->result;
    }
    /**
     * SDK alias for completionResult
     *
     * @param mixed $result
     *
     * @return self
     */
    public function setResult($result): self
    {
        $this->initialized['result'] = true;
        $this->result = $result;
        return $this;
    }
    /**
     * COMPLETED|FAILED|… (instance status) or SUCCESS|FAILURE (completion outcome)
     *
     * @return string|null
     */
    public function getStatus(): ?string
    {
        return $this->status;
    }
    /**
     * COMPLETED|FAILED|… (instance status) or SUCCESS|FAILURE (completion outcome)
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