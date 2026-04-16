<?php

namespace FlowCatalyst\Generated\Endpoint;

class GetApiAdminRolesBySourceBySource extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    protected $source;
    /**
     * @param string $source Role source (CODE, DATABASE, SDK)
     */
    public function __construct(string $source)
    {
        $this->source = $source;
    }
    use \FlowCatalyst\Generated\Runtime\Client\EndpointTrait;
    public function getMethod(): string
    {
        return 'GET';
    }
    public function getUri(): string
    {
        return str_replace(['{source}'], [$this->source], '/api/roles/by-source/{source}');
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
     * @throws \FlowCatalyst\Generated\Exception\GetApiAdminRolesBySourceBySourceBadRequestException
     *
     * @return null|\FlowCatalyst\Generated\Model\RoleResponse[]
     */
    protected function transformResponseBody(\Psr\Http\Message\ResponseInterface $response, \Symfony\Component\Serializer\SerializerInterface $serializer, ?string $contentType = null)
    {
        $status = $response->getStatusCode();
        $body = (string) $response->getBody();
        if (is_null($contentType) === false && (200 === $status && mb_strpos(strtolower($contentType), 'application/json') !== false)) {
            return $serializer->deserialize($body, 'FlowCatalyst\Generated\Model\RoleResponse[]', 'json');
        }
        if (400 === $status) {
            throw new \FlowCatalyst\Generated\Exception\GetApiAdminRolesBySourceBySourceBadRequestException($response);
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}