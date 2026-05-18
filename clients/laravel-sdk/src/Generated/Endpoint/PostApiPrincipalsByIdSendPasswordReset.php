<?php

namespace FlowCatalyst\Generated\Endpoint;

class PostApiPrincipalsByIdSendPasswordReset extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    protected $id;
    /**
     * Sends the same single-use email as the user-initiated
     * `/auth/password-reset/request` flow. The user clicks the link and sets
     * their own password; the admin never sees or handles the password.
     *
     * Rejects OIDC-federated users (they manage credentials at their IDP) and
     * users without an email address.
     * @param string $id Principal ID
     */
    public function __construct(string $id)
    {
        $this->id = $id;
    }
    use \FlowCatalyst\Generated\Runtime\Client\EndpointTrait;
    public function getMethod(): string
    {
        return 'POST';
    }
    public function getUri(): string
    {
        return str_replace(['{id}'], [$this->id], '/api/principals/{id}/send-password-reset');
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
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdSendPasswordResetBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdSendPasswordResetForbiddenException
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdSendPasswordResetNotFoundException
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
            throw new \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdSendPasswordResetBadRequestException($response);
        }
        if (403 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdSendPasswordResetForbiddenException($response);
        }
        if (404 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdSendPasswordResetNotFoundException($response);
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}