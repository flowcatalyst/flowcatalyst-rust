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
class CreateIdpRoleMappingRequestNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\CreateIdpRoleMappingRequest::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\CreateIdpRoleMappingRequest::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\CreateIdpRoleMappingRequest();
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
            unset($data['$schema']);
        }
        elseif (\array_key_exists('$schema', $data) && $data['$schema'] === null) {
            $object->setDollarSchema(null);
        }
        if (\array_key_exists('idpRoleName', $data) && $data['idpRoleName'] !== null) {
            $object->setIdpRoleName($data['idpRoleName']);
            unset($data['idpRoleName']);
        }
        elseif (\array_key_exists('idpRoleName', $data) && $data['idpRoleName'] === null) {
            $object->setIdpRoleName(null);
        }
        if (\array_key_exists('idpType', $data) && $data['idpType'] !== null) {
            $object->setIdpType($data['idpType']);
            unset($data['idpType']);
        }
        elseif (\array_key_exists('idpType', $data) && $data['idpType'] === null) {
            $object->setIdpType(null);
        }
        if (\array_key_exists('platformRoleName', $data) && $data['platformRoleName'] !== null) {
            $object->setPlatformRoleName($data['platformRoleName']);
            unset($data['platformRoleName']);
        }
        elseif (\array_key_exists('platformRoleName', $data) && $data['platformRoleName'] === null) {
            $object->setPlatformRoleName(null);
        }
        foreach ($data as $key => $value) {
            if (preg_match('/.*/', (string) $key)) {
                $object[$key] = $value;
            }
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $dataArray['idpRoleName'] = $data->getIdpRoleName();
        $dataArray['idpType'] = $data->getIdpType();
        $dataArray['platformRoleName'] = $data->getPlatformRoleName();
        foreach ($data as $key => $value) {
            if (preg_match('/.*/', (string) $key)) {
                $dataArray[$key] = $value;
            }
        }
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\CreateIdpRoleMappingRequest::class => false];
    }
}