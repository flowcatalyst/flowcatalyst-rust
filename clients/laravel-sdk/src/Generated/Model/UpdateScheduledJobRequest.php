<?php

namespace FlowCatalyst\Generated\Model;

class UpdateScheduledJobRequest extends \ArrayObject
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
     * @var bool|null
     */
    protected $concurrent;
    /**
     * @var list<string>|null
     */
    protected $crons;
    /**
     * @var int|null
     */
    protected $deliveryMaxAttempts;
    /**
     * @var string|null
     */
    protected $description;
    /**
     * @var string|null
     */
    protected $name;
    /**
     * @var mixed|null
     */
    protected $payload;
    /**
     * @var string|null
     */
    protected $targetUrl;
    /**
     * @var int|null
     */
    protected $timeoutSeconds;
    /**
     * @var string|null
     */
    protected $timezone;
    /**
     * @var bool|null
     */
    protected $tracksCompletion;
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
     * @return bool|null
     */
    public function getConcurrent(): ?bool
    {
        return $this->concurrent;
    }
    /**
     * @param bool|null $concurrent
     *
     * @return self
     */
    public function setConcurrent(?bool $concurrent): self
    {
        $this->initialized['concurrent'] = true;
        $this->concurrent = $concurrent;
        return $this;
    }
    /**
     * @return list<string>|null
     */
    public function getCrons(): ?array
    {
        return $this->crons;
    }
    /**
     * @param list<string>|null $crons
     *
     * @return self
     */
    public function setCrons(?array $crons): self
    {
        $this->initialized['crons'] = true;
        $this->crons = $crons;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getDeliveryMaxAttempts(): ?int
    {
        return $this->deliveryMaxAttempts;
    }
    /**
     * @param int|null $deliveryMaxAttempts
     *
     * @return self
     */
    public function setDeliveryMaxAttempts(?int $deliveryMaxAttempts): self
    {
        $this->initialized['deliveryMaxAttempts'] = true;
        $this->deliveryMaxAttempts = $deliveryMaxAttempts;
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
     * @return mixed
     */
    public function getPayload()
    {
        return $this->payload;
    }
    /**
     * @param mixed $payload
     *
     * @return self
     */
    public function setPayload($payload): self
    {
        $this->initialized['payload'] = true;
        $this->payload = $payload;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getTargetUrl(): ?string
    {
        return $this->targetUrl;
    }
    /**
     * @param string|null $targetUrl
     *
     * @return self
     */
    public function setTargetUrl(?string $targetUrl): self
    {
        $this->initialized['targetUrl'] = true;
        $this->targetUrl = $targetUrl;
        return $this;
    }
    /**
     * @return int|null
     */
    public function getTimeoutSeconds(): ?int
    {
        return $this->timeoutSeconds;
    }
    /**
     * @param int|null $timeoutSeconds
     *
     * @return self
     */
    public function setTimeoutSeconds(?int $timeoutSeconds): self
    {
        $this->initialized['timeoutSeconds'] = true;
        $this->timeoutSeconds = $timeoutSeconds;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getTimezone(): ?string
    {
        return $this->timezone;
    }
    /**
     * @param string|null $timezone
     *
     * @return self
     */
    public function setTimezone(?string $timezone): self
    {
        $this->initialized['timezone'] = true;
        $this->timezone = $timezone;
        return $this;
    }
    /**
     * @return bool|null
     */
    public function getTracksCompletion(): ?bool
    {
        return $this->tracksCompletion;
    }
    /**
     * @param bool|null $tracksCompletion
     *
     * @return self
     */
    public function setTracksCompletion(?bool $tracksCompletion): self
    {
        $this->initialized['tracksCompletion'] = true;
        $this->tracksCompletion = $tracksCompletion;
        return $this;
    }
}