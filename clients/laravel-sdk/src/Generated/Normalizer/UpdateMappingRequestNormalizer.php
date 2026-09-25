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
class UpdateMappingRequestNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\UpdateMappingRequest::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\UpdateMappingRequest::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\UpdateMappingRequest();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('rememberDeviceEnabled', $data) && \is_int($data['rememberDeviceEnabled'])) {
            $data['rememberDeviceEnabled'] = (bool) $data['rememberDeviceEnabled'];
        }
        if (\array_key_exists('require2fa', $data) && \is_int($data['require2fa'])) {
            $data['require2fa'] = (bool) $data['require2fa'];
        }
        if (\array_key_exists('$schema', $data) && $data['$schema'] !== null) {
            $object->setDollarSchema($data['$schema']);
            unset($data['$schema']);
        }
        elseif (\array_key_exists('$schema', $data) && $data['$schema'] === null) {
            $object->setDollarSchema(null);
        }
        if (\array_key_exists('additionalClientIds', $data) && $data['additionalClientIds'] !== null) {
            $values = [];
            foreach ($data['additionalClientIds'] as $value) {
                $values[] = $value;
            }
            $object->setAdditionalClientIds($values);
            unset($data['additionalClientIds']);
        }
        elseif (\array_key_exists('additionalClientIds', $data) && $data['additionalClientIds'] === null) {
            $object->setAdditionalClientIds(null);
        }
        if (\array_key_exists('allowed2faMethods', $data) && $data['allowed2faMethods'] !== null) {
            $values_1 = [];
            foreach ($data['allowed2faMethods'] as $value_1) {
                $values_1[] = $value_1;
            }
            $object->setAllowed2faMethods($values_1);
            unset($data['allowed2faMethods']);
        }
        elseif (\array_key_exists('allowed2faMethods', $data) && $data['allowed2faMethods'] === null) {
            $object->setAllowed2faMethods(null);
        }
        if (\array_key_exists('grantedClientIds', $data) && $data['grantedClientIds'] !== null) {
            $values_2 = [];
            foreach ($data['grantedClientIds'] as $value_2) {
                $values_2[] = $value_2;
            }
            $object->setGrantedClientIds($values_2);
            unset($data['grantedClientIds']);
        }
        elseif (\array_key_exists('grantedClientIds', $data) && $data['grantedClientIds'] === null) {
            $object->setGrantedClientIds(null);
        }
        if (\array_key_exists('primaryClientId', $data) && $data['primaryClientId'] !== null) {
            $object->setPrimaryClientId($data['primaryClientId']);
            unset($data['primaryClientId']);
        }
        elseif (\array_key_exists('primaryClientId', $data) && $data['primaryClientId'] === null) {
            $object->setPrimaryClientId(null);
        }
        if (\array_key_exists('rememberDeviceDays', $data) && $data['rememberDeviceDays'] !== null) {
            $object->setRememberDeviceDays($data['rememberDeviceDays']);
            unset($data['rememberDeviceDays']);
        }
        elseif (\array_key_exists('rememberDeviceDays', $data) && $data['rememberDeviceDays'] === null) {
            $object->setRememberDeviceDays(null);
        }
        if (\array_key_exists('rememberDeviceEnabled', $data) && $data['rememberDeviceEnabled'] !== null) {
            $object->setRememberDeviceEnabled($data['rememberDeviceEnabled']);
            unset($data['rememberDeviceEnabled']);
        }
        elseif (\array_key_exists('rememberDeviceEnabled', $data) && $data['rememberDeviceEnabled'] === null) {
            $object->setRememberDeviceEnabled(null);
        }
        if (\array_key_exists('require2fa', $data) && $data['require2fa'] !== null) {
            $object->setRequire2fa($data['require2fa']);
            unset($data['require2fa']);
        }
        elseif (\array_key_exists('require2fa', $data) && $data['require2fa'] === null) {
            $object->setRequire2fa(null);
        }
        if (\array_key_exists('requiredOidcTenantId', $data) && $data['requiredOidcTenantId'] !== null) {
            $object->setRequiredOidcTenantId($data['requiredOidcTenantId']);
            unset($data['requiredOidcTenantId']);
        }
        elseif (\array_key_exists('requiredOidcTenantId', $data) && $data['requiredOidcTenantId'] === null) {
            $object->setRequiredOidcTenantId(null);
        }
        foreach ($data as $key => $value_3) {
            if (preg_match('/.*/', (string) $key)) {
                $object[$key] = $value_3;
            }
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        if ($data->isInitialized('additionalClientIds') && null !== $data->getAdditionalClientIds()) {
            $values = [];
            foreach ($data->getAdditionalClientIds() as $value) {
                $values[] = $value;
            }
            $dataArray['additionalClientIds'] = $values;
        }
        if ($data->isInitialized('allowed2faMethods') && null !== $data->getAllowed2faMethods()) {
            $values_1 = [];
            foreach ($data->getAllowed2faMethods() as $value_1) {
                $values_1[] = $value_1;
            }
            $dataArray['allowed2faMethods'] = $values_1;
        }
        if ($data->isInitialized('grantedClientIds') && null !== $data->getGrantedClientIds()) {
            $values_2 = [];
            foreach ($data->getGrantedClientIds() as $value_2) {
                $values_2[] = $value_2;
            }
            $dataArray['grantedClientIds'] = $values_2;
        }
        if ($data->isInitialized('primaryClientId') && null !== $data->getPrimaryClientId()) {
            $dataArray['primaryClientId'] = $data->getPrimaryClientId();
        }
        if ($data->isInitialized('rememberDeviceDays') && null !== $data->getRememberDeviceDays()) {
            $dataArray['rememberDeviceDays'] = $data->getRememberDeviceDays();
        }
        if ($data->isInitialized('rememberDeviceEnabled') && null !== $data->getRememberDeviceEnabled()) {
            $dataArray['rememberDeviceEnabled'] = $data->getRememberDeviceEnabled();
        }
        if ($data->isInitialized('require2fa') && null !== $data->getRequire2fa()) {
            $dataArray['require2fa'] = $data->getRequire2fa();
        }
        if ($data->isInitialized('requiredOidcTenantId') && null !== $data->getRequiredOidcTenantId()) {
            $dataArray['requiredOidcTenantId'] = $data->getRequiredOidcTenantId();
        }
        foreach ($data as $key => $value_3) {
            if (preg_match('/.*/', (string) $key)) {
                $dataArray[$key] = $value_3;
            }
        }
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\UpdateMappingRequest::class => false];
    }
}