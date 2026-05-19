<?php

namespace FlowCatalyst\Generated\Endpoint;

class DeleteApiRolesByNamePermissionsByPermission extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    protected $roleName;
    protected $permission;
    /**
     * @param string $roleName Role name (code) or ID
     * @param string $permission Permission to revoke
     */
    public function __construct(string $roleName, string $permission)
    {
        $this->roleName = $roleName;
        $this->permission = $permission;
    }
    use \FlowCatalyst\Generated\Runtime\Client\EndpointTrait;
    public function getMethod(): string
    {
        return 'DELETE';
    }
    public function getUri(): string
    {
        return str_replace(['{roleName}', '{permission}'], [$this->roleName, $this->permission], '/api/roles/{roleName}/permissions/{permission}');
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
     * @throws \FlowCatalyst\Generated\Exception\DeleteApiRolesByNamePermissionsByPermissionNotFoundException
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
            throw new \FlowCatalyst\Generated\Exception\DeleteApiRolesByNamePermissionsByPermissionNotFoundException($response);
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}