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
class SetApplicationAccessResponseNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\SetApplicationAccessResponse::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\SetApplicationAccessResponse::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\SetApplicationAccessResponse();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('allApplications', $data) && \is_int($data['allApplications'])) {
            $data['allApplications'] = (bool) $data['allApplications'];
        }
        if (\array_key_exists('$schema', $data) && $data['$schema'] !== null) {
            $object->setDollarSchema($data['$schema']);
        }
        elseif (\array_key_exists('$schema', $data) && $data['$schema'] === null) {
            $object->setDollarSchema(null);
        }
        if (\array_key_exists('added', $data) && $data['added'] !== null) {
            $object->setAdded($data['added']);
        }
        elseif (\array_key_exists('added', $data) && $data['added'] === null) {
            $object->setAdded(null);
        }
        if (\array_key_exists('allApplications', $data) && $data['allApplications'] !== null) {
            $object->setAllApplications($data['allApplications']);
        }
        elseif (\array_key_exists('allApplications', $data) && $data['allApplications'] === null) {
            $object->setAllApplications(null);
        }
        if (\array_key_exists('applications', $data) && $data['applications'] !== null) {
            $values = [];
            foreach ($data['applications'] as $value) {
                $values[] = $this->denormalizer->denormalize($value, \FlowCatalyst\Generated\Model\ApplicationAccessResponse::class, 'json', $context);
            }
            $object->setApplications($values);
        }
        elseif (\array_key_exists('applications', $data) && $data['applications'] === null) {
            $object->setApplications(null);
        }
        if (\array_key_exists('removed', $data) && $data['removed'] !== null) {
            $object->setRemoved($data['removed']);
        }
        elseif (\array_key_exists('removed', $data) && $data['removed'] === null) {
            $object->setRemoved(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $dataArray['added'] = $data->getAdded();
        $dataArray['allApplications'] = $data->getAllApplications();
        $values = [];
        foreach ($data->getApplications() as $value) {
            $values[] = $this->normalizer->normalize($value, 'json', $context);
        }
        $dataArray['applications'] = $values;
        $dataArray['removed'] = $data->getRemoved();
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\SetApplicationAccessResponse::class => false];
    }
}