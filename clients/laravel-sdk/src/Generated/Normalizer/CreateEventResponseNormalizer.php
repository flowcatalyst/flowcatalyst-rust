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
class CreateEventResponseNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\CreateEventResponse::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\CreateEventResponse::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\CreateEventResponse();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('isDuplicate', $data) && \is_int($data['isDuplicate'])) {
            $data['isDuplicate'] = (bool) $data['isDuplicate'];
        }
        if (\array_key_exists('$schema', $data) && $data['$schema'] !== null) {
            $object->setDollarSchema($data['$schema']);
        }
        elseif (\array_key_exists('$schema', $data) && $data['$schema'] === null) {
            $object->setDollarSchema(null);
        }
        if (\array_key_exists('dispatchJobCount', $data) && $data['dispatchJobCount'] !== null) {
            $object->setDispatchJobCount($data['dispatchJobCount']);
        }
        elseif (\array_key_exists('dispatchJobCount', $data) && $data['dispatchJobCount'] === null) {
            $object->setDispatchJobCount(null);
        }
        if (\array_key_exists('event', $data) && $data['event'] !== null) {
            $object->setEvent($this->denormalizer->denormalize($data['event'], \FlowCatalyst\Generated\Model\CreatedEvent::class, 'json', $context));
        }
        elseif (\array_key_exists('event', $data) && $data['event'] === null) {
            $object->setEvent(null);
        }
        if (\array_key_exists('isDuplicate', $data) && $data['isDuplicate'] !== null) {
            $object->setIsDuplicate($data['isDuplicate']);
        }
        elseif (\array_key_exists('isDuplicate', $data) && $data['isDuplicate'] === null) {
            $object->setIsDuplicate(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $dataArray['dispatchJobCount'] = $data->getDispatchJobCount();
        $dataArray['event'] = $this->normalizer->normalize($data->getEvent(), 'json', $context);
        $dataArray['isDuplicate'] = $data->getIsDuplicate();
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\CreateEventResponse::class => false];
    }
}