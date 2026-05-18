<?php

namespace FlowCatalyst\Generated\Endpoint;

class PostApiClientsByIdDeactivate extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    protected $id;
    /**
     * Deactivates/soft-deletes a client. Requires a reason.
     * @param string $id Client ID
     * @param null|\FlowCatalyst\Generated\Model\StatusChangeRequest $requestBody
     */
    public function __construct(string $id, ?\FlowCatalyst\Generated\Model\StatusChangeRequest $requestBody = null)
    {
        $this->id = $id;
        $this->body = $requestBody;
    }
    use \FlowCatalyst\Generated\Runtime\Client\EndpointTrait;
    public function getMethod(): string
    {
        return 'POST';
    }
    public function getUri(): string
    {
        return str_replace(['{id}'], [$this->id], '/api/clients/{id}/deactivate');
    }
    public function getBody(\Symfony\Component\Serializer\SerializerInterface $serializer, $streamFactory = null): array
    {
        if ($this->body instanceof \FlowCatalyst\Generated\Model\StatusChangeRequest) {
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
     * @throws \FlowCatalyst\Generated\Exception\PostApiClientsByIdDeactivateForbiddenException
     * @throws \FlowCatalyst\Generated\Exception\PostApiClientsByIdDeactivateNotFoundException
     *
     * @return null|\FlowCatalyst\Generated\Model\StatusChangeResponse
     */
    protected function transformResponseBody(\Psr\Http\Message\ResponseInterface $response, \Symfony\Component\Serializer\SerializerInterface $serializer, ?string $contentType = null)
    {
        $status = $response->getStatusCode();
        $body = (string) $response->getBody();
        if (is_null($contentType) === false && (200 === $status && mb_strpos(strtolower($contentType), 'application/json') !== false)) {
            return $serializer->deserialize($body, 'FlowCatalyst\Generated\Model\StatusChangeResponse', 'json');
        }
        if (403 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiClientsByIdDeactivateForbiddenException($response);
        }
        if (404 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiClientsByIdDeactivateNotFoundException($response);
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}