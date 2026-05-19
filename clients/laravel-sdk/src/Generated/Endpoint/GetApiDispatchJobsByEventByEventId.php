<?php

namespace FlowCatalyst\Generated\Endpoint;

class GetApiDispatchJobsByEventByEventId extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    protected $eventId;
    /**
     * @param string $eventId Event ID
     */
    public function __construct(string $eventId)
    {
        $this->eventId = $eventId;
    }
    use \FlowCatalyst\Generated\Runtime\Client\EndpointTrait;
    public function getMethod(): string
    {
        return 'GET';
    }
    public function getUri(): string
    {
        return str_replace(['{eventId}'], [$this->eventId], '/api/dispatch-jobs/by-event/{eventId}');
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
     * @return null|\FlowCatalyst\Generated\Model\DispatchJobResponse[]
     */
    protected function transformResponseBody(\Psr\Http\Message\ResponseInterface $response, \Symfony\Component\Serializer\SerializerInterface $serializer, ?string $contentType = null)
    {
        $status = $response->getStatusCode();
        $body = (string) $response->getBody();
        if (is_null($contentType) === false && (200 === $status && mb_strpos(strtolower($contentType), 'application/json') !== false)) {
            return $serializer->deserialize($body, 'FlowCatalyst\Generated\Model\DispatchJobResponse[]', 'json');
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}