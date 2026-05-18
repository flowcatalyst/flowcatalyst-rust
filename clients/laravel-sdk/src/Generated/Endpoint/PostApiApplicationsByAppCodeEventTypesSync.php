<?php

namespace FlowCatalyst\Generated\Endpoint;

class PostApiApplicationsByAppCodeEventTypesSync extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    protected $app_code;
    /**
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncEventTypesRequest $requestBody
     * @param array{
     *    "removeUnlisted"?: bool, //Remove API-sourced event types not in list
     * } $queryParameters
     */
    public function __construct(string $appCode, ?\FlowCatalyst\Generated\Model\SyncEventTypesRequest $requestBody = null, array $queryParameters = [])
    {
        $this->app_code = $appCode;
        $this->body = $requestBody;
        $this->queryParameters = $queryParameters;
    }
    use \FlowCatalyst\Generated\Runtime\Client\EndpointTrait;
    public function getMethod(): string
    {
        return 'POST';
    }
    public function getUri(): string
    {
        return str_replace(['{app_code}'], [$this->app_code], '/api/applications/{app_code}/event-types/sync');
    }
    public function getBody(\Symfony\Component\Serializer\SerializerInterface $serializer, $streamFactory = null): array
    {
        if ($this->body instanceof \FlowCatalyst\Generated\Model\SyncEventTypesRequest) {
            return [['Content-Type' => ['application/json']], $serializer->serialize($this->body, 'json')];
        }
        return [[], null];
    }
    public function getExtraHeaders(): array
    {
        return ['Accept' => ['application/json']];
    }
    protected function getQueryOptionsResolver(): \Symfony\Component\OptionsResolver\OptionsResolver
    {
        $optionsResolver = parent::getQueryOptionsResolver();
        $optionsResolver->setDefined(['removeUnlisted']);
        $optionsResolver->setRequired([]);
        $optionsResolver->setDefaults([]);
        $optionsResolver->addAllowedTypes('removeUnlisted', ['bool']);
        return $optionsResolver;
    }
    /**
     * {@inheritdoc}
     *
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeEventTypesSyncBadRequestException
     *
     * @return null|\FlowCatalyst\Generated\Model\SyncResultResponse
     */
    protected function transformResponseBody(\Psr\Http\Message\ResponseInterface $response, \Symfony\Component\Serializer\SerializerInterface $serializer, ?string $contentType = null)
    {
        $status = $response->getStatusCode();
        $body = (string) $response->getBody();
        if (is_null($contentType) === false && (200 === $status && mb_strpos(strtolower($contentType), 'application/json') !== false)) {
            return $serializer->deserialize($body, 'FlowCatalyst\Generated\Model\SyncResultResponse', 'json');
        }
        if (400 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeEventTypesSyncBadRequestException($response);
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}