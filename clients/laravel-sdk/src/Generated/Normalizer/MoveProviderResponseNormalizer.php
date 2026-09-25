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
class MoveProviderResponseNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\MoveProviderResponse::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\MoveProviderResponse::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\MoveProviderResponse();
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
        if (\array_key_exists('emailDomain', $data) && $data['emailDomain'] !== null) {
            $object->setEmailDomain($data['emailDomain']);
        }
        elseif (\array_key_exists('emailDomain', $data) && $data['emailDomain'] === null) {
            $object->setEmailDomain(null);
        }
        if (\array_key_exists('fromIdentityProviderId', $data) && $data['fromIdentityProviderId'] !== null) {
            $object->setFromIdentityProviderId($data['fromIdentityProviderId']);
        }
        elseif (\array_key_exists('fromIdentityProviderId', $data) && $data['fromIdentityProviderId'] === null) {
            $object->setFromIdentityProviderId(null);
        }
        if (\array_key_exists('mappingId', $data) && $data['mappingId'] !== null) {
            $object->setMappingId($data['mappingId']);
        }
        elseif (\array_key_exists('mappingId', $data) && $data['mappingId'] === null) {
            $object->setMappingId(null);
        }
        if (\array_key_exists('toIdentityProviderId', $data) && $data['toIdentityProviderId'] !== null) {
            $object->setToIdentityProviderId($data['toIdentityProviderId']);
        }
        elseif (\array_key_exists('toIdentityProviderId', $data) && $data['toIdentityProviderId'] === null) {
            $object->setToIdentityProviderId(null);
        }
        if (\array_key_exists('usersReset', $data) && $data['usersReset'] !== null) {
            $object->setUsersReset($data['usersReset']);
        }
        elseif (\array_key_exists('usersReset', $data) && $data['usersReset'] === null) {
            $object->setUsersReset(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $dataArray['emailDomain'] = $data->getEmailDomain();
        $dataArray['fromIdentityProviderId'] = $data->getFromIdentityProviderId();
        $dataArray['mappingId'] = $data->getMappingId();
        $dataArray['toIdentityProviderId'] = $data->getToIdentityProviderId();
        $dataArray['usersReset'] = $data->getUsersReset();
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\MoveProviderResponse::class => false];
    }
}