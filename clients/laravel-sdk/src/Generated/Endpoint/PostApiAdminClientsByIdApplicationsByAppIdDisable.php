<?php

namespace FlowCatalyst\Generated\Endpoint;

class PostApiAdminClientsByIdApplicationsByAppIdDisable extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    protected $id;
    protected $application_id;
    /**
     * @param string $id Client ID
     * @param string $applicationId Application ID
     */
    public function __construct(string $id, string $applicationId)
    {
        $this->id = $id;
        $this->application_id = $applicationId;
    }
    use \FlowCatalyst\Generated\Runtime\Client\EndpointTrait;
    public function getMethod(): string
    {
        return 'POST';
    }
    public function getUri(): string
    {
        return str_replace(['{id}', '{application_id}'], [$this->id, $this->application_id], '/api/clients/{id}/applications/{application_id}/disable');
    }
    public function getBody(\Symfony\Component\Serializer\SerializerInterface $serializer, $streamFactory = null): array
    {
        return [[], null];
    }
    /**
     * {@inheritdoc}
     *
     * @throws \FlowCatalyst\Generated\Exception\PostApiAdminClientsByIdApplicationsByAppIdDisableNotFoundException
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
            throw new \FlowCatalyst\Generated\Exception\PostApiAdminClientsByIdApplicationsByAppIdDisableNotFoundException($response);
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}