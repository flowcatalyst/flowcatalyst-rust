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
class QueueConfigNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\QueueConfig::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\QueueConfig::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\QueueConfig();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('connections', $data) && $data['connections'] !== null) {
            $object->setConnections($data['connections']);
        }
        elseif (\array_key_exists('connections', $data) && $data['connections'] === null) {
            $object->setConnections(null);
        }
        if (\array_key_exists('queueName', $data) && $data['queueName'] !== null) {
            $object->setQueueName($data['queueName']);
        }
        elseif (\array_key_exists('queueName', $data) && $data['queueName'] === null) {
            $object->setQueueName(null);
        }
        if (\array_key_exists('queueUri', $data) && $data['queueUri'] !== null) {
            $object->setQueueUri($data['queueUri']);
        }
        elseif (\array_key_exists('queueUri', $data) && $data['queueUri'] === null) {
            $object->setQueueUri(null);
        }
        if (\array_key_exists('visibilityTimeout', $data) && $data['visibilityTimeout'] !== null) {
            $object->setVisibilityTimeout($data['visibilityTimeout']);
        }
        elseif (\array_key_exists('visibilityTimeout', $data) && $data['visibilityTimeout'] === null) {
            $object->setVisibilityTimeout(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $dataArray['connections'] = $data->getConnections();
        $dataArray['queueName'] = $data->getQueueName();
        $dataArray['queueUri'] = $data->getQueueUri();
        $dataArray['visibilityTimeout'] = $data->getVisibilityTimeout();
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\QueueConfig::class => false];
    }
}