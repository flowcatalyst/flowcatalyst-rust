<?php

namespace FlowCatalyst\Generated\Endpoint;

class PostApiScheduledJobsInstancesByIdLog extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    protected $instanceId;
    /**
     * @param string $instanceId Instance ID
     * @param null|\FlowCatalyst\Generated\Model\InstanceLogRequest $requestBody
     */
    public function __construct(string $instanceId, ?\FlowCatalyst\Generated\Model\InstanceLogRequest $requestBody = null)
    {
        $this->instanceId = $instanceId;
        $this->body = $requestBody;
    }
    use \FlowCatalyst\Generated\Runtime\Client\EndpointTrait;
    public function getMethod(): string
    {
        return 'POST';
    }
    public function getUri(): string
    {
        return str_replace(['{instanceId}'], [$this->instanceId], '/api/scheduled-jobs/instances/{instanceId}/log');
    }
    public function getBody(\Symfony\Component\Serializer\SerializerInterface $serializer, $streamFactory = null): array
    {
        if ($this->body instanceof \FlowCatalyst\Generated\Model\InstanceLogRequest) {
            return [['Content-Type' => ['application/json']], $serializer->serialize($this->body, 'json')];
        }
        return [[], null];
    }
    /**
     * {@inheritdoc}
     *
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsInstancesByIdLogForbiddenException
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsInstancesByIdLogNotFoundException
     *
     * @return null
     */
    protected function transformResponseBody(\Psr\Http\Message\ResponseInterface $response, \Symfony\Component\Serializer\SerializerInterface $serializer, ?string $contentType = null)
    {
        $status = $response->getStatusCode();
        $body = (string) $response->getBody();
        if (202 === $status) {
            return null;
        }
        if (403 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiScheduledJobsInstancesByIdLogForbiddenException($response);
        }
        if (404 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiScheduledJobsInstancesByIdLogNotFoundException($response);
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}