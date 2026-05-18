<?php

namespace FlowCatalyst\Generated\Endpoint;

class PostApiPrincipalsByIdResetPassword extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    protected $id;
    /**
     * Resets the password for an internal auth user. Does not work for OIDC users.
     * @param string $id Principal ID
     * @param null|\FlowCatalyst\Generated\Model\ResetPasswordRequest $requestBody
     */
    public function __construct(string $id, ?\FlowCatalyst\Generated\Model\ResetPasswordRequest $requestBody = null)
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
        return str_replace(['{id}'], [$this->id], '/api/principals/{id}/reset-password');
    }
    public function getBody(\Symfony\Component\Serializer\SerializerInterface $serializer, $streamFactory = null): array
    {
        if ($this->body instanceof \FlowCatalyst\Generated\Model\ResetPasswordRequest) {
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
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdResetPasswordBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdResetPasswordForbiddenException
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdResetPasswordNotFoundException
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
        if (400 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdResetPasswordBadRequestException($response);
        }
        if (403 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdResetPasswordForbiddenException($response);
        }
        if (404 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdResetPasswordNotFoundException($response);
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}