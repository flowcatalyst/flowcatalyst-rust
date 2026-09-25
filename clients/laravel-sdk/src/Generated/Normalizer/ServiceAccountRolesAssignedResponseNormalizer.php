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
class ServiceAccountRolesAssignedResponseNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\ServiceAccountRolesAssignedResponse::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\ServiceAccountRolesAssignedResponse::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\ServiceAccountRolesAssignedResponse();
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
        if (\array_key_exists('addedRoles', $data) && $data['addedRoles'] !== null) {
            $values = [];
            foreach ($data['addedRoles'] as $value) {
                $values[] = $value;
            }
            $object->setAddedRoles($values);
        }
        elseif (\array_key_exists('addedRoles', $data) && $data['addedRoles'] === null) {
            $object->setAddedRoles(null);
        }
        if (\array_key_exists('removedRoles', $data) && $data['removedRoles'] !== null) {
            $values_1 = [];
            foreach ($data['removedRoles'] as $value_1) {
                $values_1[] = $value_1;
            }
            $object->setRemovedRoles($values_1);
        }
        elseif (\array_key_exists('removedRoles', $data) && $data['removedRoles'] === null) {
            $object->setRemovedRoles(null);
        }
        if (\array_key_exists('roles', $data) && $data['roles'] !== null) {
            $values_2 = [];
            foreach ($data['roles'] as $value_2) {
                $values_2[] = $this->denormalizer->denormalize($value_2, \FlowCatalyst\Generated\Model\RoleAssignmentDTO::class, 'json', $context);
            }
            $object->setRoles($values_2);
        }
        elseif (\array_key_exists('roles', $data) && $data['roles'] === null) {
            $object->setRoles(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $values = [];
        foreach ($data->getAddedRoles() as $value) {
            $values[] = $value;
        }
        $dataArray['addedRoles'] = $values;
        $values_1 = [];
        foreach ($data->getRemovedRoles() as $value_1) {
            $values_1[] = $value_1;
        }
        $dataArray['removedRoles'] = $values_1;
        $values_2 = [];
        foreach ($data->getRoles() as $value_2) {
            $values_2[] = $this->normalizer->normalize($value_2, 'json', $context);
        }
        $dataArray['roles'] = $values_2;
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\ServiceAccountRolesAssignedResponse::class => false];
    }
}