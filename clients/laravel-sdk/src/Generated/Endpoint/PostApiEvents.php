<?php

namespace FlowCatalyst\Generated\Endpoint;

class PostApiEvents extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    /**
     * Creates a new event in the event store. If a deduplicationId is provided and
     * an event with that ID already exists, the existing event is returned (idempotent operation).
     * Dispatch jobs are automatically created for matching subscriptions.
     * @param null|\FlowCatalyst\Generated\Model\CreateEventRequest $requestBody
     */
    public function __construct(?\FlowCatalyst\Generated\Model\CreateEventRequest $requestBody = null)
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
        return '/api/events';
    }
    public function getBody(\Symfony\Component\Serializer\SerializerInterface $serializer, $streamFactory = null): array
    {
        if ($this->body instanceof \FlowCatalyst\Generated\Model\CreateEventRequest) {
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
     * @throws \FlowCatalyst\Generated\Exception\PostApiEventsBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiEventsForbiddenException
     *
     * @return null|\FlowCatalyst\Generated\Model\CreateEventResponse
     */
    protected function transformResponseBody(\Psr\Http\Message\ResponseInterface $response, \Symfony\Component\Serializer\SerializerInterface $serializer, ?string $contentType = null)
    {
        $status = $response->getStatusCode();
        $body = (string) $response->getBody();
        if (is_null($contentType) === false && (200 === $status && mb_strpos(strtolower($contentType), 'application/json') !== false)) {
            return $serializer->deserialize($body, 'FlowCatalyst\Generated\Model\CreateEventResponse', 'json');
        }
        if (is_null($contentType) === false && (201 === $status && mb_strpos(strtolower($contentType), 'application/json') !== false)) {
            return $serializer->deserialize($body, 'FlowCatalyst\Generated\Model\CreateEventResponse', 'json');
        }
        if (400 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiEventsBadRequestException($response);
        }
        if (403 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiEventsForbiddenException($response);
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}