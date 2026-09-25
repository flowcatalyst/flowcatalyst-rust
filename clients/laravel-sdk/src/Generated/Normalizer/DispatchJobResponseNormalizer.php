<?php

namespace FlowCatalyst\Generated\Normalizer;

use Jane\Component\JsonSchemaRuntime\Reference;
use FlowCatalyst\Generated\Runtime\Normalizer\CheckArray;
use FlowCatalyst\Generated\Runtime\Normalizer\ValidatorTrait;
use Symfony\Component\Serializer\Normalizer\DenormalizerAwareInterface;
use Symfony\Component\Serializer\Normalizer\DenormalizerAwareTrait;
use Symfony\Component\Serializer\Normalizer\DenormalizerInterface;
use Symfony\Component\Serializer\Normalizer\NormalizerAwareInterface;
use Symfony\Component\Serializer\Normalizer\NormalizerAwareTrait;
use Symfony\Component\Serializer\Normalizer\NormalizerInterface;
class DispatchJobResponseNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\DispatchJobResponse::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\DispatchJobResponse::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\DispatchJobResponse();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('dataOnly', $data) && \is_int($data['dataOnly'])) {
            $data['dataOnly'] = (bool) $data['dataOnly'];
        }
        if (\array_key_exists('$schema', $data) && $data['$schema'] !== null) {
            $object->setDollarSchema($data['$schema']);
        }
        elseif (\array_key_exists('$schema', $data) && $data['$schema'] === null) {
            $object->setDollarSchema(null);
        }
        if (\array_key_exists('attemptCount', $data) && $data['attemptCount'] !== null) {
            $object->setAttemptCount($data['attemptCount']);
        }
        elseif (\array_key_exists('attemptCount', $data) && $data['attemptCount'] === null) {
            $object->setAttemptCount(null);
        }
        if (\array_key_exists('attempts', $data) && $data['attempts'] !== null) {
            $values = [];
            foreach ($data['attempts'] as $value) {
                $values[] = $this->denormalizer->denormalize($value, \FlowCatalyst\Generated\Model\AttemptDTO::class, 'json', $context);
            }
            $object->setAttempts($values);
        }
        elseif (\array_key_exists('attempts', $data) && $data['attempts'] === null) {
            $object->setAttempts(null);
        }
        if (\array_key_exists('clientId', $data) && $data['clientId'] !== null) {
            $object->setClientId($data['clientId']);
        }
        elseif (\array_key_exists('clientId', $data) && $data['clientId'] === null) {
            $object->setClientId(null);
        }
        if (\array_key_exists('code', $data) && $data['code'] !== null) {
            $object->setCode($data['code']);
        }
        elseif (\array_key_exists('code', $data) && $data['code'] === null) {
            $object->setCode(null);
        }
        if (\array_key_exists('completedAt', $data) && $data['completedAt'] !== null) {
            $object->setCompletedAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['completedAt']));
        }
        elseif (\array_key_exists('completedAt', $data) && $data['completedAt'] === null) {
            $object->setCompletedAt(null);
        }
        if (\array_key_exists('correlationId', $data) && $data['correlationId'] !== null) {
            $object->setCorrelationId($data['correlationId']);
        }
        elseif (\array_key_exists('correlationId', $data) && $data['correlationId'] === null) {
            $object->setCorrelationId(null);
        }
        if (\array_key_exists('createdAt', $data) && $data['createdAt'] !== null) {
            $object->setCreatedAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['createdAt']));
        }
        elseif (\array_key_exists('createdAt', $data) && $data['createdAt'] === null) {
            $object->setCreatedAt(null);
        }
        if (\array_key_exists('dataOnly', $data) && $data['dataOnly'] !== null) {
            $object->setDataOnly($data['dataOnly']);
        }
        elseif (\array_key_exists('dataOnly', $data) && $data['dataOnly'] === null) {
            $object->setDataOnly(null);
        }
        if (\array_key_exists('dispatchPoolId', $data) && $data['dispatchPoolId'] !== null) {
            $object->setDispatchPoolId($data['dispatchPoolId']);
        }
        elseif (\array_key_exists('dispatchPoolId', $data) && $data['dispatchPoolId'] === null) {
            $object->setDispatchPoolId(null);
        }
        if (\array_key_exists('durationMillis', $data) && $data['durationMillis'] !== null) {
            $object->setDurationMillis($data['durationMillis']);
        }
        elseif (\array_key_exists('durationMillis', $data) && $data['durationMillis'] === null) {
            $object->setDurationMillis(null);
        }
        if (\array_key_exists('eventId', $data) && $data['eventId'] !== null) {
            $object->setEventId($data['eventId']);
        }
        elseif (\array_key_exists('eventId', $data) && $data['eventId'] === null) {
            $object->setEventId(null);
        }
        if (\array_key_exists('expiresAt', $data) && $data['expiresAt'] !== null) {
            $object->setExpiresAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['expiresAt']));
        }
        elseif (\array_key_exists('expiresAt', $data) && $data['expiresAt'] === null) {
            $object->setExpiresAt(null);
        }
        if (\array_key_exists('externalId', $data) && $data['externalId'] !== null) {
            $object->setExternalId($data['externalId']);
        }
        elseif (\array_key_exists('externalId', $data) && $data['externalId'] === null) {
            $object->setExternalId(null);
        }
        if (\array_key_exists('id', $data) && $data['id'] !== null) {
            $object->setId($data['id']);
        }
        elseif (\array_key_exists('id', $data) && $data['id'] === null) {
            $object->setId(null);
        }
        if (\array_key_exists('idempotencyKey', $data) && $data['idempotencyKey'] !== null) {
            $object->setIdempotencyKey($data['idempotencyKey']);
        }
        elseif (\array_key_exists('idempotencyKey', $data) && $data['idempotencyKey'] === null) {
            $object->setIdempotencyKey(null);
        }
        if (\array_key_exists('kind', $data) && $data['kind'] !== null) {
            $object->setKind($data['kind']);
        }
        elseif (\array_key_exists('kind', $data) && $data['kind'] === null) {
            $object->setKind(null);
        }
        if (\array_key_exists('lastAttemptAt', $data) && $data['lastAttemptAt'] !== null) {
            $object->setLastAttemptAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['lastAttemptAt']));
        }
        elseif (\array_key_exists('lastAttemptAt', $data) && $data['lastAttemptAt'] === null) {
            $object->setLastAttemptAt(null);
        }
        if (\array_key_exists('lastError', $data) && $data['lastError'] !== null) {
            $object->setLastError($data['lastError']);
        }
        elseif (\array_key_exists('lastError', $data) && $data['lastError'] === null) {
            $object->setLastError(null);
        }
        if (\array_key_exists('maxRetries', $data) && $data['maxRetries'] !== null) {
            $object->setMaxRetries($data['maxRetries']);
        }
        elseif (\array_key_exists('maxRetries', $data) && $data['maxRetries'] === null) {
            $object->setMaxRetries(null);
        }
        if (\array_key_exists('messageGroup', $data) && $data['messageGroup'] !== null) {
            $object->setMessageGroup($data['messageGroup']);
        }
        elseif (\array_key_exists('messageGroup', $data) && $data['messageGroup'] === null) {
            $object->setMessageGroup(null);
        }
        if (\array_key_exists('metadata', $data) && $data['metadata'] !== null) {
            $values_1 = [];
            foreach ($data['metadata'] as $value_1) {
                $values_1[] = $this->denormalizer->denormalize($value_1, \FlowCatalyst\Generated\Model\MetadataDTO::class, 'json', $context);
            }
            $object->setMetadata($values_1);
        }
        elseif (\array_key_exists('metadata', $data) && $data['metadata'] === null) {
            $object->setMetadata(null);
        }
        if (\array_key_exists('mode', $data) && $data['mode'] !== null) {
            $object->setMode($data['mode']);
        }
        elseif (\array_key_exists('mode', $data) && $data['mode'] === null) {
            $object->setMode(null);
        }
        if (\array_key_exists('payload', $data) && $data['payload'] !== null) {
            $object->setPayload($data['payload']);
        }
        elseif (\array_key_exists('payload', $data) && $data['payload'] === null) {
            $object->setPayload(null);
        }
        if (\array_key_exists('payloadContentType', $data) && $data['payloadContentType'] !== null) {
            $object->setPayloadContentType($data['payloadContentType']);
        }
        elseif (\array_key_exists('payloadContentType', $data) && $data['payloadContentType'] === null) {
            $object->setPayloadContentType(null);
        }
        if (\array_key_exists('protocol', $data) && $data['protocol'] !== null) {
            $object->setProtocol($data['protocol']);
        }
        elseif (\array_key_exists('protocol', $data) && $data['protocol'] === null) {
            $object->setProtocol(null);
        }
        if (\array_key_exists('retryStrategy', $data) && $data['retryStrategy'] !== null) {
            $object->setRetryStrategy($data['retryStrategy']);
        }
        elseif (\array_key_exists('retryStrategy', $data) && $data['retryStrategy'] === null) {
            $object->setRetryStrategy(null);
        }
        if (\array_key_exists('scheduledFor', $data) && $data['scheduledFor'] !== null) {
            $object->setScheduledFor(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['scheduledFor']));
        }
        elseif (\array_key_exists('scheduledFor', $data) && $data['scheduledFor'] === null) {
            $object->setScheduledFor(null);
        }
        if (\array_key_exists('schemaId', $data) && $data['schemaId'] !== null) {
            $object->setSchemaId($data['schemaId']);
        }
        elseif (\array_key_exists('schemaId', $data) && $data['schemaId'] === null) {
            $object->setSchemaId(null);
        }
        if (\array_key_exists('sequence', $data) && $data['sequence'] !== null) {
            $object->setSequence($data['sequence']);
        }
        elseif (\array_key_exists('sequence', $data) && $data['sequence'] === null) {
            $object->setSequence(null);
        }
        if (\array_key_exists('serviceAccountId', $data) && $data['serviceAccountId'] !== null) {
            $object->setServiceAccountId($data['serviceAccountId']);
        }
        elseif (\array_key_exists('serviceAccountId', $data) && $data['serviceAccountId'] === null) {
            $object->setServiceAccountId(null);
        }
        if (\array_key_exists('source', $data) && $data['source'] !== null) {
            $object->setSource($data['source']);
        }
        elseif (\array_key_exists('source', $data) && $data['source'] === null) {
            $object->setSource(null);
        }
        if (\array_key_exists('status', $data) && $data['status'] !== null) {
            $object->setStatus($data['status']);
        }
        elseif (\array_key_exists('status', $data) && $data['status'] === null) {
            $object->setStatus(null);
        }
        if (\array_key_exists('subject', $data) && $data['subject'] !== null) {
            $object->setSubject($data['subject']);
        }
        elseif (\array_key_exists('subject', $data) && $data['subject'] === null) {
            $object->setSubject(null);
        }
        if (\array_key_exists('subscriptionId', $data) && $data['subscriptionId'] !== null) {
            $object->setSubscriptionId($data['subscriptionId']);
        }
        elseif (\array_key_exists('subscriptionId', $data) && $data['subscriptionId'] === null) {
            $object->setSubscriptionId(null);
        }
        if (\array_key_exists('targetUrl', $data) && $data['targetUrl'] !== null) {
            $object->setTargetUrl($data['targetUrl']);
        }
        elseif (\array_key_exists('targetUrl', $data) && $data['targetUrl'] === null) {
            $object->setTargetUrl(null);
        }
        if (\array_key_exists('timeoutSeconds', $data) && $data['timeoutSeconds'] !== null) {
            $object->setTimeoutSeconds($data['timeoutSeconds']);
        }
        elseif (\array_key_exists('timeoutSeconds', $data) && $data['timeoutSeconds'] === null) {
            $object->setTimeoutSeconds(null);
        }
        if (\array_key_exists('updatedAt', $data) && $data['updatedAt'] !== null) {
            $object->setUpdatedAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['updatedAt']));
        }
        elseif (\array_key_exists('updatedAt', $data) && $data['updatedAt'] === null) {
            $object->setUpdatedAt(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $dataArray['attemptCount'] = $data->getAttemptCount();
        if ($data->isInitialized('attempts') && null !== $data->getAttempts()) {
            $values = [];
            foreach ($data->getAttempts() as $value) {
                $values[] = $this->normalizer->normalize($value, 'json', $context);
            }
            $dataArray['attempts'] = $values;
        }
        if ($data->isInitialized('clientId') && null !== $data->getClientId()) {
            $dataArray['clientId'] = $data->getClientId();
        }
        $dataArray['code'] = $data->getCode();
        if ($data->isInitialized('completedAt') && null !== $data->getCompletedAt()) {
            $dataArray['completedAt'] = $data->getCompletedAt()->format('Y-m-d\TH:i:sP');
        }
        if ($data->isInitialized('correlationId') && null !== $data->getCorrelationId()) {
            $dataArray['correlationId'] = $data->getCorrelationId();
        }
        $dataArray['createdAt'] = $data->getCreatedAt()->format('Y-m-d\TH:i:sP');
        $dataArray['dataOnly'] = $data->getDataOnly();
        if ($data->isInitialized('dispatchPoolId') && null !== $data->getDispatchPoolId()) {
            $dataArray['dispatchPoolId'] = $data->getDispatchPoolId();
        }
        if ($data->isInitialized('durationMillis') && null !== $data->getDurationMillis()) {
            $dataArray['durationMillis'] = $data->getDurationMillis();
        }
        if ($data->isInitialized('eventId') && null !== $data->getEventId()) {
            $dataArray['eventId'] = $data->getEventId();
        }
        if ($data->isInitialized('expiresAt') && null !== $data->getExpiresAt()) {
            $dataArray['expiresAt'] = $data->getExpiresAt()->format('Y-m-d\TH:i:sP');
        }
        if ($data->isInitialized('externalId') && null !== $data->getExternalId()) {
            $dataArray['externalId'] = $data->getExternalId();
        }
        $dataArray['id'] = $data->getId();
        if ($data->isInitialized('idempotencyKey') && null !== $data->getIdempotencyKey()) {
            $dataArray['idempotencyKey'] = $data->getIdempotencyKey();
        }
        $dataArray['kind'] = $data->getKind();
        if ($data->isInitialized('lastAttemptAt') && null !== $data->getLastAttemptAt()) {
            $dataArray['lastAttemptAt'] = $data->getLastAttemptAt()->format('Y-m-d\TH:i:sP');
        }
        if ($data->isInitialized('lastError') && null !== $data->getLastError()) {
            $dataArray['lastError'] = $data->getLastError();
        }
        $dataArray['maxRetries'] = $data->getMaxRetries();
        if ($data->isInitialized('messageGroup') && null !== $data->getMessageGroup()) {
            $dataArray['messageGroup'] = $data->getMessageGroup();
        }
        if ($data->isInitialized('metadata') && null !== $data->getMetadata()) {
            $values_1 = [];
            foreach ($data->getMetadata() as $value_1) {
                $values_1[] = $this->normalizer->normalize($value_1, 'json', $context);
            }
            $dataArray['metadata'] = $values_1;
        }
        $dataArray['mode'] = $data->getMode();
        if ($data->isInitialized('payload') && null !== $data->getPayload()) {
            $dataArray['payload'] = $data->getPayload();
        }
        $dataArray['payloadContentType'] = $data->getPayloadContentType();
        $dataArray['protocol'] = $data->getProtocol();
        $dataArray['retryStrategy'] = $data->getRetryStrategy();
        if ($data->isInitialized('scheduledFor') && null !== $data->getScheduledFor()) {
            $dataArray['scheduledFor'] = $data->getScheduledFor()->format('Y-m-d\TH:i:sP');
        }
        if ($data->isInitialized('schemaId') && null !== $data->getSchemaId()) {
            $dataArray['schemaId'] = $data->getSchemaId();
        }
        $dataArray['sequence'] = $data->getSequence();
        if ($data->isInitialized('serviceAccountId') && null !== $data->getServiceAccountId()) {
            $dataArray['serviceAccountId'] = $data->getServiceAccountId();
        }
        if ($data->isInitialized('source') && null !== $data->getSource()) {
            $dataArray['source'] = $data->getSource();
        }
        $dataArray['status'] = $data->getStatus();
        if ($data->isInitialized('subject') && null !== $data->getSubject()) {
            $dataArray['subject'] = $data->getSubject();
        }
        if ($data->isInitialized('subscriptionId') && null !== $data->getSubscriptionId()) {
            $dataArray['subscriptionId'] = $data->getSubscriptionId();
        }
        $dataArray['targetUrl'] = $data->getTargetUrl();
        $dataArray['timeoutSeconds'] = $data->getTimeoutSeconds();
        $dataArray['updatedAt'] = $data->getUpdatedAt()->format('Y-m-d\TH:i:sP');
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\DispatchJobResponse::class => false];
    }
}