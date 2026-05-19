<?php

namespace FlowCatalyst\Generated\Endpoint;

class PutApiRolesByName extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    protected $roleName;
    /**
     * @param string $roleName Role name (code) or ID
     * @param null|\FlowCatalyst\Generated\Model\UpdateRoleRequest $requestBody
     */
    public function __construct(string $roleName, ?\FlowCatalyst\Generated\Model\UpdateRoleRequest $requestBody = null)
    {
        $this->roleName = $roleName;
        $this->body = $requestBody;
    }
    use \FlowCatalyst\Generated\Runtime\Client\EndpointTrait;
    public function getMethod(): string
    {
        return 'PUT';
    }
    public function getUri(): string
    {
        return str_replace(['{roleName}'], [$this->roleName], '/api/roles/{roleName}');
    }
    public function getBody(\Symfony\Component\Serializer\SerializerInterface $serializer, $streamFactory = null): array
    {
        if ($this->body instanceof \FlowCatalyst\Generated\Model\UpdateRoleRequest) {
            return [['Content-Type' => ['application/json']], $serializer->serialize($this->body, 'json')];
        }
        return [[], null];
    }
    /**
     * {@inheritdoc}
     *
     * @throws \FlowCatalyst\Generated\Exception\PutApiRolesByNameNotFoundException
     *
     * @return null
     */
    protected function transformResponseBody(\Psr\Http\Message\ResponseInterface $response, \Symfony\Component\Serializer\SerializerInterface $serializer, ?string $contentType = null)
    {
        $status = $response->getStatusCode();
        $body = (string) $response->getBody();
        if (204 === $status) {
            return null;
        }
        if (404 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PutApiRolesByNameNotFoundException($response);
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}