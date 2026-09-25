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
class AuditLogResponseNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\AuditLogResponse::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\AuditLogResponse::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\AuditLogResponse();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('$schema', $data) && $data['$schema'] !== null) {
            $object->setDollarSchema($data['$schema']);
        }
        elseif (\array_key_exists('$schema', $data) && $data['$schema'] === null) {
            $object->setDollarSchema(null);
        }
        if (\array_key_exists('applicationId', $data) && $data['applicationId'] !== null) {
            $object->setApplicationId($data['applicationId']);
        }
        elseif (\array_key_exists('applicationId', $data) && $data['applicationId'] === null) {
            $object->setApplicationId(null);
        }
        if (\array_key_exists('clientId', $data) && $data['clientId'] !== null) {
            $object->setClientId($data['clientId']);
        }
        elseif (\array_key_exists('clientId', $data) && $data['clientId'] === null) {
            $object->setClientId(null);
        }
        if (\array_key_exists('entityId', $data) && $data['entityId'] !== null) {
            $object->setEntityId($data['entityId']);
        }
        elseif (\array_key_exists('entityId', $data) && $data['entityId'] === null) {
            $object->setEntityId(null);
        }
        if (\array_key_exists('entityType', $data) && $data['entityType'] !== null) {
            $object->setEntityType($data['entityType']);
        }
        elseif (\array_key_exists('entityType', $data) && $data['entityType'] === null) {
            $object->setEntityType(null);
        }
        if (\array_key_exists('id', $data) && $data['id'] !== null) {
            $object->setId($data['id']);
        }
        elseif (\array_key_exists('id', $data) && $data['id'] === null) {
            $object->setId(null);
        }
        if (\array_key_exists('operation', $data) && $data['operation'] !== null) {
            $object->setOperation($data['operation']);
        }
        elseif (\array_key_exists('operation', $data) && $data['operation'] === null) {
            $object->setOperation(null);
        }
        if (\array_key_exists('operationJson', $data) && $data['operationJson'] !== null) {
            $object->setOperationJson($data['operationJson']);
        }
        elseif (\array_key_exists('operationJson', $data) && $data['operationJson'] === null) {
            $object->setOperationJson(null);
        }
        if (\array_key_exists('performedAt', $data) && $data['performedAt'] !== null) {
            $object->setPerformedAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['performedAt']));
        }
        elseif (\array_key_exists('performedAt', $data) && $data['performedAt'] === null) {
            $object->setPerformedAt(null);
        }
        if (\array_key_exists('principalId', $data) && $data['principalId'] !== null) {
            $object->setPrincipalId($data['principalId']);
        }
        elseif (\array_key_exists('principalId', $data) && $data['principalId'] === null) {
            $object->setPrincipalId(null);
        }
        if (\array_key_exists('principalName', $data) && $data['principalName'] !== null) {
            $object->setPrincipalName($data['principalName']);
        }
        elseif (\array_key_exists('principalName', $data) && $data['principalName'] === null) {
            $object->setPrincipalName(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        if ($data->isInitialized('applicationId') && null !== $data->getApplicationId()) {
            $dataArray['applicationId'] = $data->getApplicationId();
        }
        if ($data->isInitialized('clientId') && null !== $data->getClientId()) {
            $dataArray['clientId'] = $data->getClientId();
        }
        $dataArray['entityId'] = $data->getEntityId();
        $dataArray['entityType'] = $data->getEntityType();
        $dataArray['id'] = $data->getId();
        $dataArray['operation'] = $data->getOperation();
        if ($data->isInitialized('operationJson') && null !== $data->getOperationJson()) {
            $dataArray['operationJson'] = $data->getOperationJson();
        }
        $dataArray['performedAt'] = $data->getPerformedAt()->format('Y-m-d\TH:i:sP');
        if ($data->isInitialized('principalId') && null !== $data->getPrincipalId()) {
            $dataArray['principalId'] = $data->getPrincipalId();
        }
        if ($data->isInitialized('principalName') && null !== $data->getPrincipalName()) {
            $dataArray['principalName'] = $data->getPrincipalName();
        }
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\AuditLogResponse::class => false];
    }
}