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
class RoleAssignmentDTONormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\RoleAssignmentDTO::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\RoleAssignmentDTO::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\RoleAssignmentDTO();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('assignedAt', $data) && $data['assignedAt'] !== null) {
            $object->setAssignedAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['assignedAt']));
        }
        elseif (\array_key_exists('assignedAt', $data) && $data['assignedAt'] === null) {
            $object->setAssignedAt(null);
        }
        if (\array_key_exists('assignedBy', $data) && $data['assignedBy'] !== null) {
            $object->setAssignedBy($data['assignedBy']);
        }
        elseif (\array_key_exists('assignedBy', $data) && $data['assignedBy'] === null) {
            $object->setAssignedBy(null);
        }
        if (\array_key_exists('assignmentSource', $data) && $data['assignmentSource'] !== null) {
            $object->setAssignmentSource($data['assignmentSource']);
        }
        elseif (\array_key_exists('assignmentSource', $data) && $data['assignmentSource'] === null) {
            $object->setAssignmentSource(null);
        }
        if (\array_key_exists('clientId', $data) && $data['clientId'] !== null) {
            $object->setClientId($data['clientId']);
        }
        elseif (\array_key_exists('clientId', $data) && $data['clientId'] === null) {
            $object->setClientId(null);
        }
        if (\array_key_exists('roleName', $data) && $data['roleName'] !== null) {
            $object->setRoleName($data['roleName']);
        }
        elseif (\array_key_exists('roleName', $data) && $data['roleName'] === null) {
            $object->setRoleName(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $dataArray['assignedAt'] = $data->getAssignedAt()->format('Y-m-d\TH:i:sP');
        if ($data->isInitialized('assignedBy') && null !== $data->getAssignedBy()) {
            $dataArray['assignedBy'] = $data->getAssignedBy();
        }
        if ($data->isInitialized('assignmentSource') && null !== $data->getAssignmentSource()) {
            $dataArray['assignmentSource'] = $data->getAssignmentSource();
        }
        if ($data->isInitialized('clientId') && null !== $data->getClientId()) {
            $dataArray['clientId'] = $data->getClientId();
        }
        $dataArray['roleName'] = $data->getRoleName();
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\RoleAssignmentDTO::class => false];
    }
}