<?php

namespace FlowCatalyst\Generated\Endpoint;

class GetApiAuditLogsEntityByEntityTypeByEntityId extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    protected $entityType;
    protected $entityId;
    /**
     * @param string $entityType Entity type
     * @param string $entityId Entity ID
     */
    public function __construct(string $entityType, string $entityId)
    {
        $this->entityType = $entityType;
        $this->entityId = $entityId;
    }
    use \FlowCatalyst\Generated\Runtime\Client\EndpointTrait;
    public function getMethod(): string
    {
        return 'GET';
    }
    public function getUri(): string
    {
        return str_replace(['{entityType}', '{entityId}'], [$this->entityType, $this->entityId], '/api/audit-logs/entity/{entityType}/{entityId}');
    }
    public function getBody(\Symfony\Component\Serializer\SerializerInterface $serializer, $streamFactory = null): array
    {
        return [[], null];
    }
    public function getExtraHeaders(): array
    {
        return ['Accept' => ['application/json']];
    }
    /**
     * {@inheritdoc}
     *
     *
     * @return null|\FlowCatalyst\Generated\Model\EntityAuditLogsResponse
     */
    protected function transformResponseBody(\Psr\Http\Message\ResponseInterface $response, \Symfony\Component\Serializer\SerializerInterface $serializer, ?string $contentType = null)
    {
        $status = $response->getStatusCode();
        $body = (string) $response->getBody();
        if (is_null($contentType) === false && (200 === $status && mb_strpos(strtolower($contentType), 'application/json') !== false)) {
            return $serializer->deserialize($body, 'FlowCatalyst\Generated\Model\EntityAuditLogsResponse', 'json');
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}