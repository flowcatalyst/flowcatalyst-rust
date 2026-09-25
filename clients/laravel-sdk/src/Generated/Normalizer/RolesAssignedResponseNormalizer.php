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
class RolesAssignedResponseNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\RolesAssignedResponse::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\RolesAssignedResponse::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\RolesAssignedResponse();
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
        if (\array_key_exists('added', $data) && $data['added'] !== null) {
            $values = [];
            foreach ($data['added'] as $value) {
                $values[] = $value;
            }
            $object->setAdded($values);
        }
        elseif (\array_key_exists('added', $data) && $data['added'] === null) {
            $object->setAdded(null);
        }
        if (\array_key_exists('removed', $data) && $data['removed'] !== null) {
            $values_1 = [];
            foreach ($data['removed'] as $value_1) {
                $values_1[] = $value_1;
            }
            $object->setRemoved($values_1);
        }
        elseif (\array_key_exists('removed', $data) && $data['removed'] === null) {
            $object->setRemoved(null);
        }
        if (\array_key_exists('roles', $data) && $data['roles'] !== null) {
            $values_2 = [];
            foreach ($data['roles'] as $value_2) {
                $values_2[] = $this->denormalizer->denormalize($value_2, \FlowCatalyst\Generated\Model\PrincipalRoleAssignmentDTO::class, 'json', $context);
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
        foreach ($data->getAdded() as $value) {
            $values[] = $value;
        }
        $dataArray['added'] = $values;
        $values_1 = [];
        foreach ($data->getRemoved() as $value_1) {
            $values_1[] = $value_1;
        }
        $dataArray['removed'] = $values_1;
        $values_2 = [];
        foreach ($data->getRoles() as $value_2) {
            $values_2[] = $this->normalizer->normalize($value_2, 'json', $context);
        }
        $dataArray['roles'] = $values_2;
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\RolesAssignedResponse::class => false];
    }
}