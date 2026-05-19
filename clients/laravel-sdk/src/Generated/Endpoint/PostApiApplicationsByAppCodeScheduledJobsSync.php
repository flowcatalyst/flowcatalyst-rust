<?php

namespace FlowCatalyst\Generated\Endpoint;

class PostApiApplicationsByAppCodeScheduledJobsSync extends \FlowCatalyst\Generated\Runtime\Client\BaseEndpoint implements \FlowCatalyst\Generated\Runtime\Client\Endpoint
{
    protected $appCode;
    /**
     * Body specifies the target client (or null for platform-scoped). Caller
     * must have access to that client (or be anchor for platform-scoped).
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncScheduledJobsRequest $requestBody
     */
    public function __construct(string $appCode, ?\FlowCatalyst\Generated\Model\SyncScheduledJobsRequest $requestBody = null)
    {
        $this->appCode = $appCode;
        $this->body = $requestBody;
    }
    use \FlowCatalyst\Generated\Runtime\Client\EndpointTrait;
    public function getMethod(): string
    {
        return 'POST';
    }
    public function getUri(): string
    {
        return str_replace(['{appCode}'], [$this->appCode], '/api/applications/{appCode}/scheduled-jobs/sync');
    }
    public function getBody(\Symfony\Component\Serializer\SerializerInterface $serializer, $streamFactory = null): array
    {
        if ($this->body instanceof \FlowCatalyst\Generated\Model\SyncScheduledJobsRequest) {
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
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeScheduledJobsSyncBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeScheduledJobsSyncForbiddenException
     *
     * @return null|\FlowCatalyst\Generated\Model\SyncScheduledJobsResultResponse
     */
    protected function transformResponseBody(\Psr\Http\Message\ResponseInterface $response, \Symfony\Component\Serializer\SerializerInterface $serializer, ?string $contentType = null)
    {
        $status = $response->getStatusCode();
        $body = (string) $response->getBody();
        if (is_null($contentType) === false && (200 === $status && mb_strpos(strtolower($contentType), 'application/json') !== false)) {
            return $serializer->deserialize($body, 'FlowCatalyst\Generated\Model\SyncScheduledJobsResultResponse', 'json');
        }
        if (400 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeScheduledJobsSyncBadRequestException($response);
        }
        if (403 === $status) {
            throw new \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeScheduledJobsSyncForbiddenException($response);
        }
    }
    public function getAuthenticationScopes(): array
    {
        return ['bearer_auth'];
    }
}