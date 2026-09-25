<?php

namespace FlowCatalyst\Generated\Endpoint;

class ListAuditLogs extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    /**
     * @param array{
     *    "after"?: string, //Opaque cursor from a previous page's nextCursor
     *    "pageSize"?: int, //Page size (default 50, capped at 200)
     *    "entityType"?: string,
     *    "entityId"?: string,
     *    "principalId"?: string,
     *    "operation"?: string,
     *    "applicationIds"?: string, //CSV of application ids
     *    "clientIds"?: string, //CSV of client ids
     * } $queryParameters
     */
    public function __construct(array $queryParameters = [])
    {
        $this->queryParameters = $queryParameters;
    }
    use \FlowCatalyst\Generated\Runtime\Client\EndpointTrait;
    public function getMethod(): string
    {
        return 'GET';
    }
    public function getUri(): string
    {
        return '/api/audit-logs';
    }
    public function getBody(\Symfony\Component\Serializer\SerializerInterface $serializer, $streamFactory = null): array
    {
        return [[], null];
    }
    public function getExtraHeaders(): array
    {
        return ['Accept' => ['application/json']];
    }
    protected function getQueryOptionsResolver(): \Symfony\Component\OptionsResolver\OptionsResolver
    {
        $optionsResolver = parent::getQueryOptionsResolver();
        $optionsResolver->setDefined(['after', 'pageSize', 'entityType', 'entityId', 'principalId', 'operation', 'applicationIds', 'clientIds']);
        $optionsResolver->setRequired([]);
        $optionsResolver->setDefaults([]);
        $optionsResolver->addAllowedTypes('after', ['string']);
        $optionsResolver->addAllowedTypes('pageSize', ['int']);
        $optionsResolver->addAllowedTypes('entityType', ['string']);
        $optionsResolver->addAllowedTypes('entityId', ['string']);
        $optionsResolver->addAllowedTypes('principalId', ['string']);
        $optionsResolver->addAllowedTypes('operation', ['string']);
        $optionsResolver->addAllowedTypes('applicationIds', ['string']);
        $optionsResolver->addAllowedTypes('clientIds', ['string']);
        return $optionsResolver;
    }
    /**
     * {@inheritdoc}
     *
     *
     * @return null|\FlowCatalyst\Generated\Model\AuditLogListResponse|\FlowCatalyst\Generated\Model\ErrorModel
     */
    protected function transformResponseBody(\Psr\Http\Message\ResponseInterface $response, \Symfony\Component\Serializer\SerializerInterface $serializer, ?string $contentType = null)
    {
        $status = $response->getStatusCode();
        $body = (string) $response->getBody();
        if (is_null($contentType) === false && (200 === $status && mb_strpos(strtolower($contentType), 'application/json') !== false)) {
            return $serializer->deserialize($body, 'FlowCatalyst\Generated\Model\AuditLogListResponse', 'json');
        }
        if (mb_strpos(strtolower($contentType), 'application/json') !== false) {
            return $serializer->deserialize($body, 'FlowCatalyst\Generated\Model\ErrorModel', 'json');
        }
    }
    public function getAuthenticationScopes(): array
    {
        return [];
    }
}