<?php

namespace FlowCatalyst\Generated\Endpoint;

class PostApiApplicationsByAppCodeOpenapiSync extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    protected $app_code;
    /**
     * Versioned: the prior CURRENT (if any) is flipped to ARCHIVED with computed
     * change-notes; the incoming document becomes the new CURRENT. Re-sending an
     * unchanged spec is a no-op (returns `unchanged: true`).
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncOpenApiSpecRequest $requestBody
     */
    public function __construct(string $appCode, ?\FlowCatalyst\Generated\Model\SyncOpenApiSpecRequest $requestBody = null)
    {
        $this->app_code = $appCode;
        $this->body = $requestBody;
    }
    use \FlowCatalyst\Generated\Runtime\Client\EndpointTrait;
    public function getMethod(): string
    {
        return 'POST';
    }
    public function getUri(): string
    {
        return str_replace(['{app_code}'], [$this->app_code], '/api/applications/{app_code}/openapi/sync');
    }
    public function getBody(\Symfony\Component\Serializer\SerializerInterface $serializer, $streamFactory = null): array
    {
        if ($this->body instanceof \FlowCatalyst\Generated\Model\SyncOpenApiSpecRequest) {
            return [['Content-Type' => ['application/json']], $serializer->serialize($this->body, 'json')];
        }
        return [[], null];
    }
    public function getExtraHeaders(): array
    {
        return ['Accept' => ['application/json']];
    }
    /**
     * {@inheritdoc}
     *
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeOpenapiSyncBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeOpenapiSyncForbiddenException
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeOpenapiSyncNotFoundException
     *
     * @return null|\FlowCatalyst\Generated\Model\SyncOpenApiSpecResponse
     */
    protected function transformResponseBody(\Psr\Http\Message\ResponseInterface $response, \Symfony\Component\Serializer\SerializerInterface $serializer, ?string $contentType = null)
    {
        $status = $response->getStatusCode();
        $body = (string) $response->getBody();
        if (is_null($contentType) === false && (200 === $status && mb_strpos(strtolower($contentType), 'application/json') !== false)) {
            return $serializer->deserialize($body, 'FlowCatalyst\Generated\Model\SyncOpenApiSpecResponse', 'json');
        }
        if (400 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeOpenapiSyncBadRequestException($response);
        }
        if (403 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeOpenapiSyncForbiddenException($response);
        }
        if (404 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeOpenapiSyncNotFoundException($response);
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}