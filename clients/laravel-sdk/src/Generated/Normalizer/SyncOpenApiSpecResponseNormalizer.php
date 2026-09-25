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
class SyncOpenApiSpecResponseNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\SyncOpenApiSpecResponse::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\SyncOpenApiSpecResponse::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\SyncOpenApiSpecResponse();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('hasBreaking', $data) && \is_int($data['hasBreaking'])) {
            $data['hasBreaking'] = (bool) $data['hasBreaking'];
        }
        if (\array_key_exists('unchanged', $data) && \is_int($data['unchanged'])) {
            $data['unchanged'] = (bool) $data['unchanged'];
        }
        if (\array_key_exists('$schema', $data) && $data['$schema'] !== null) {
            $object->setDollarSchema($data['$schema']);
        }
        elseif (\array_key_exists('$schema', $data) && $data['$schema'] === null) {
            $object->setDollarSchema(null);
        }
        if (\array_key_exists('applicationCode', $data) && $data['applicationCode'] !== null) {
            $object->setApplicationCode($data['applicationCode']);
        }
        elseif (\array_key_exists('applicationCode', $data) && $data['applicationCode'] === null) {
            $object->setApplicationCode(null);
        }
        if (\array_key_exists('archivedPriorVersion', $data) && $data['archivedPriorVersion'] !== null) {
            $object->setArchivedPriorVersion($data['archivedPriorVersion']);
        }
        elseif (\array_key_exists('archivedPriorVersion', $data) && $data['archivedPriorVersion'] === null) {
            $object->setArchivedPriorVersion(null);
        }
        if (\array_key_exists('hasBreaking', $data) && $data['hasBreaking'] !== null) {
            $object->setHasBreaking($data['hasBreaking']);
        }
        elseif (\array_key_exists('hasBreaking', $data) && $data['hasBreaking'] === null) {
            $object->setHasBreaking(null);
        }
        if (\array_key_exists('specId', $data) && $data['specId'] !== null) {
            $object->setSpecId($data['specId']);
        }
        elseif (\array_key_exists('specId', $data) && $data['specId'] === null) {
            $object->setSpecId(null);
        }
        if (\array_key_exists('status', $data) && $data['status'] !== null) {
            $object->setStatus($data['status']);
        }
        elseif (\array_key_exists('status', $data) && $data['status'] === null) {
            $object->setStatus(null);
        }
        if (\array_key_exists('unchanged', $data) && $data['unchanged'] !== null) {
            $object->setUnchanged($data['unchanged']);
        }
        elseif (\array_key_exists('unchanged', $data) && $data['unchanged'] === null) {
            $object->setUnchanged(null);
        }
        if (\array_key_exists('version', $data) && $data['version'] !== null) {
            $object->setVersion($data['version']);
        }
        elseif (\array_key_exists('version', $data) && $data['version'] === null) {
            $object->setVersion(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $dataArray['applicationCode'] = $data->getApplicationCode();
        if ($data->isInitialized('archivedPriorVersion') && null !== $data->getArchivedPriorVersion()) {
            $dataArray['archivedPriorVersion'] = $data->getArchivedPriorVersion();
        }
        $dataArray['hasBreaking'] = $data->getHasBreaking();
        $dataArray['specId'] = $data->getSpecId();
        $dataArray['status'] = $data->getStatus();
        $dataArray['unchanged'] = $data->getUnchanged();
        $dataArray['version'] = $data->getVersion();
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\SyncOpenApiSpecResponse::class => false];
    }
}