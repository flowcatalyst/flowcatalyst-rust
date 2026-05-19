<?php

namespace FlowCatalyst\Generated\Endpoint;

class PostApiRolesByNamePermissions extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    protected $roleName;
    /**
     * @param string $roleName Role name (code) or ID
     * @param null|\FlowCatalyst\Generated\Model\GrantPermissionRequest $requestBody
     */
    public function __construct(string $roleName, ?\FlowCatalyst\Generated\Model\GrantPermissionRequest $requestBody = null)
    {
        $this->roleName = $roleName;
        $this->body = $requestBody;
    }
    use \FlowCatalyst\Generated\Runtime\Client\EndpointTrait;
    public function getMethod(): string
    {
        return 'POST';
    }
    public function getUri(): string
    {
        return str_replace(['{roleName}'], [$this->roleName], '/api/roles/{roleName}/permissions');
    }
    public function getBody(\Symfony\Component\Serializer\SerializerInterface $serializer, $streamFactory = null): array
    {
        if ($this->body instanceof \FlowCatalyst\Generated\Model\GrantPermissionRequest) {
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
     * @throws \FlowCatalyst\Generated\Exception\PostApiRolesByNamePermissionsNotFoundException
     *
     * @return null|\FlowCatalyst\Generated\Model\RoleResponse
     */
    protected function transformResponseBody(\Psr\Http\Message\ResponseInterface $response, \Symfony\Component\Serializer\SerializerInterface $serializer, ?string $contentType = null)
    {
        $status = $response->getStatusCode();
        $body = (string) $response->getBody();
        if (is_null($contentType) === false && (200 === $status && mb_strpos(strtolower($contentType), 'application/json') !== false)) {
            return $serializer->deserialize($body, 'FlowCatalyst\Generated\Model\RoleResponse', 'json');
        }
        if (404 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiRolesByNamePermissionsNotFoundException($response);
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}