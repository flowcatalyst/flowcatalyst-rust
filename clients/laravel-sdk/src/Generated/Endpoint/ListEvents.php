<?php

namespace FlowCatalyst\Generated\Endpoint;

class ListEvents extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    /**
     * @param array{
     *    "type"?: string,
     *    "source"?: string,
     *    "subject"?: string,
     *    "clientId"?: string,
     *    "principalId"?: string,
     *    "correlationId"?: string,
     *    "since"?: string, //RFC3339 timestamp
     *    "until"?: string, //RFC3339 timestamp
     *    "limit"?: int,
     *    "offset"?: int,
     *    "size"?: int, //Max rows (default 50, max 1000)
     *    "clientIds"?: string, //CSV of client ids
     *    "applications"?: string, //CSV of application codes
     *    "subdomains"?: string, //CSV of subdomains
     *    "aggregates"?: string, //CSV of aggregates
     *    "types"?: string, //CSV of event types
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
        return '/api/events';
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
        $optionsResolver->setDefined(['type', 'source', 'subject', 'clientId', 'principalId', 'correlationId', 'since', 'until', 'limit', 'offset', 'size', 'clientIds', 'applications', 'subdomains', 'aggregates', 'types']);
        $optionsResolver->setRequired([]);
        $optionsResolver->setDefaults([]);
        $optionsResolver->addAllowedTypes('type', ['string']);
        $optionsResolver->addAllowedTypes('source', ['string']);
        $optionsResolver->addAllowedTypes('subject', ['string']);
        $optionsResolver->addAllowedTypes('clientId', ['string']);
        $optionsResolver->addAllowedTypes('principalId', ['string']);
        $optionsResolver->addAllowedTypes('correlationId', ['string']);
        $optionsResolver->addAllowedTypes('since', ['string']);
        $optionsResolver->addAllowedTypes('until', ['string']);
        $optionsResolver->addAllowedTypes('limit', ['int']);
        $optionsResolver->addAllowedTypes('offset', ['int']);
        $optionsResolver->addAllowedTypes('size', ['int']);
        $optionsResolver->addAllowedTypes('clientIds', ['string']);
        $optionsResolver->addAllowedTypes('applications', ['string']);
        $optionsResolver->addAllowedTypes('subdomains', ['string']);
        $optionsResolver->addAllowedTypes('aggregates', ['string']);
        $optionsResolver->addAllowedTypes('types', ['string']);
        return $optionsResolver;
    }
    /**
     * {@inheritdoc}
     *
     *
     * @return null|\FlowCatalyst\Generated\Model\EventRead[]|\FlowCatalyst\Generated\Model\ErrorModel
     */
    protected function transformResponseBody(\Psr\Http\Message\ResponseInterface $response, \Symfony\Component\Serializer\SerializerInterface $serializer, ?string $contentType = null)
    {
        $status = $response->getStatusCode();
        $body = (string) $response->getBody();
        if (is_null($contentType) === false && (200 === $status && mb_strpos(strtolower($contentType), 'application/json') !== false)) {
            return $serializer->deserialize($body, 'FlowCatalyst\Generated\Model\EventRead[]', 'json');
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