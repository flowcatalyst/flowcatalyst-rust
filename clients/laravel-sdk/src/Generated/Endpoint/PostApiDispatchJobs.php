<?php

namespace FlowCatalyst\Generated\Endpoint;

class PostApiDispatchJobs extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    /**
     * Creates and queues a new dispatch job for webhook delivery.
     * @param null|\FlowCatalyst\Generated\Model\CreateDispatchJobRequest $requestBody
     */
    public function __construct(?\FlowCatalyst\Generated\Model\CreateDispatchJobRequest $requestBody = null)
    {
        $this->body = $requestBody;
    }
    use \FlowCatalyst\Generated\Runtime\Client\EndpointTrait;
    public function getMethod(): string
    {
        return 'POST';
    }
    public function getUri(): string
    {
        return '/api/dispatch-jobs';
    }
    public function getBody(\Symfony\Component\Serializer\SerializerInterface $serializer, $streamFactory = null): array
    {
        if ($this->body instanceof \FlowCatalyst\Generated\Model\CreateDispatchJobRequest) {
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
     * @throws \FlowCatalyst\Generated\Exception\PostApiDispatchJobsBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiDispatchJobsForbiddenException
     *
     * @return null|\FlowCatalyst\Generated\Model\CreatedResponse
     */
    protected function transformResponseBody(\Psr\Http\Message\ResponseInterface $response, \Symfony\Component\Serializer\SerializerInterface $serializer, ?string $contentType = null)
    {
        $status = $response->getStatusCode();
        $body = (string) $response->getBody();
        if (is_null($contentType) === false && (201 === $status && mb_strpos(strtolower($contentType), 'application/json') !== false)) {
            return $serializer->deserialize($body, 'FlowCatalyst\Generated\Model\CreatedResponse', 'json');
        }
        if (400 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiDispatchJobsBadRequestException($response);
        }
        if (403 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiDispatchJobsForbiddenException($response);
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}