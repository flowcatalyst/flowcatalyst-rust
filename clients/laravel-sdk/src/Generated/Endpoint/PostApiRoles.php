<?php

namespace FlowCatalyst\Generated\Endpoint;

class PostApiRoles extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateRoleRequest $requestBody
     */
    public function __construct(?\FlowCatalyst\Generated\Model\CreateRoleRequest $requestBody = null)
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
        return '/api/roles';
    }
    public function getBody(\Symfony\Component\Serializer\SerializerInterface $serializer, $streamFactory = null): array
    {
        if ($this->body instanceof \FlowCatalyst\Generated\Model\CreateRoleRequest) {
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
     * @throws \FlowCatalyst\Generated\Exception\PostApiRolesBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiRolesConflictException
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
            throw new \FlowCatalyst\Generated\Exception\PostApiRolesBadRequestException($response);
        }
        if (409 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiRolesConflictException($response);
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}