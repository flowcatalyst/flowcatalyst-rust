<?php

namespace FlowCatalyst\Generated\Model;

class AuditLogResponse
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
     * @var string|null
     */
    protected $applicationId;
    /**
     * @var string|null
     */
    protected $clientId;
    /**
     * @var string|null
     */
    protected $entityId;
    /**
     * @var string|null
     */
    protected $entityType;
    /**
     * @var string|null
     */
    protected $id;
    /**
     * @var string|null
     */
    protected $operation;
    /**
     * @var string|null
     */
    protected $operationJson;
    /**
     * @var \DateTime|null
     */
    protected $performedAt;
    /**
     * @var string|null
     */
    protected $principalId;
    /**
     * @var string|null
     */
    protected $principalName;
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
     * @return string|null
     */
    public function getApplicationId(): ?string
    {
        return $this->applicationId;
    }
    /**
     * @param string|null $applicationId
     *
     * @return self
     */
    public function setApplicationId(?string $applicationId): self
    {
        $this->initialized['applicationId'] = true;
        $this->applicationId = $applicationId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getClientId(): ?string
    {
        return $this->clientId;
    }
    /**
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
     * @return string|null
     */
    public function getEntityId(): ?string
    {
        return $this->entityId;
    }
    /**
     * @param string|null $entityId
     *
     * @return self
     */
    public function setEntityId(?string $entityId): self
    {
        $this->initialized['entityId'] = true;
        $this->entityId = $entityId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getEntityType(): ?string
    {
        return $this->entityType;
    }
    /**
     * @param string|null $entityType
     *
     * @return self
     */
    public function setEntityType(?string $entityType): self
    {
        $this->initialized['entityType'] = true;
        $this->entityType = $entityType;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getId(): ?string
    {
        return $this->id;
    }
    /**
     * @param string|null $id
     *
     * @return self
     */
    public function setId(?string $id): self
    {
        $this->initialized['id'] = true;
        $this->id = $id;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getOperation(): ?string
    {
        return $this->operation;
    }
    /**
     * @param string|null $operation
     *
     * @return self
     */
    public function setOperation(?string $operation): self
    {
        $this->initialized['operation'] = true;
        $this->operation = $operation;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getOperationJson(): ?string
    {
        return $this->operationJson;
    }
    /**
     * @param string|null $operationJson
     *
     * @return self
     */
    public function setOperationJson(?string $operationJson): self
    {
        $this->initialized['operationJson'] = true;
        $this->operationJson = $operationJson;
        return $this;
    }
    /**
     * @return \DateTime|null
     */
    public function getPerformedAt(): ?\DateTime
    {
        return $this->performedAt;
    }
    /**
     * @param \DateTime|null $performedAt
     *
     * @return self
     */
    public function setPerformedAt(?\DateTime $performedAt): self
    {
        $this->initialized['performedAt'] = true;
        $this->performedAt = $performedAt;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getPrincipalId(): ?string
    {
        return $this->principalId;
    }
    /**
     * @param string|null $principalId
     *
     * @return self
     */
    public function setPrincipalId(?string $principalId): self
    {
        $this->initialized['principalId'] = true;
        $this->principalId = $principalId;
        return $this;
    }
    /**
     * @return string|null
     */
    public function getPrincipalName(): ?string
    {
        return $this->principalName;
    }
    /**
     * @param string|null $principalName
     *
     * @return self
     */
    public function setPrincipalName(?string $principalName): self
    {
        $this->initialized['principalName'] = true;
        $this->principalName = $principalName;
        return $this;
    }
}