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
class PrincipalRoleAssignmentDTONormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\PrincipalRoleAssignmentDTO::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\PrincipalRoleAssignmentDTO::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\PrincipalRoleAssignmentDTO();
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
        if (\array_key_exists('assignmentSource', $data) && $data['assignmentSource'] !== null) {
            $object->setAssignmentSource($data['assignmentSource']);
        }
        elseif (\array_key_exists('assignmentSource', $data) && $data['assignmentSource'] === null) {
            $object->setAssignmentSource(null);
        }
        if (\array_key_exists('id', $data) && $data['id'] !== null) {
            $object->setId($data['id']);
        }
        elseif (\array_key_exists('id', $data) && $data['id'] === null) {
            $object->setId(null);
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
        $dataArray['assignmentSource'] = $data->getAssignmentSource();
        $dataArray['id'] = $data->getId();
        $dataArray['roleName'] = $data->getRoleName();
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\PrincipalRoleAssignmentDTO::class => false];
    }
}