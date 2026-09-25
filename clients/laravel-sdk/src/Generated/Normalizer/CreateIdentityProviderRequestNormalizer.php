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
class CreateIdentityProviderRequestNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\CreateIdentityProviderRequest::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\CreateIdentityProviderRequest::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\CreateIdentityProviderRequest();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('oidcMultiTenant', $data) && \is_int($data['oidcMultiTenant'])) {
            $data['oidcMultiTenant'] = (bool) $data['oidcMultiTenant'];
        }
        if (\array_key_exists('syncRolesFromIdp', $data) && \is_int($data['syncRolesFromIdp'])) {
            $data['syncRolesFromIdp'] = (bool) $data['syncRolesFromIdp'];
        }
        if (\array_key_exists('$schema', $data) && $data['$schema'] !== null) {
            $object->setDollarSchema($data['$schema']);
            unset($data['$schema']);
        }
        elseif (\array_key_exists('$schema', $data) && $data['$schema'] === null) {
            $object->setDollarSchema(null);
        }
        if (\array_key_exists('allowedEmailDomains', $data) && $data['allowedEmailDomains'] !== null) {
            $values = [];
            foreach ($data['allowedEmailDomains'] as $value) {
                $values[] = $value;
            }
            $object->setAllowedEmailDomains($values);
            unset($data['allowedEmailDomains']);
        }
        elseif (\array_key_exists('allowedEmailDomains', $data) && $data['allowedEmailDomains'] === null) {
            $object->setAllowedEmailDomains(null);
        }
        if (\array_key_exists('allowedRoleIds', $data) && $data['allowedRoleIds'] !== null) {
            $values_1 = [];
            foreach ($data['allowedRoleIds'] as $value_1) {
                $values_1[] = $value_1;
            }
            $object->setAllowedRoleIds($values_1);
            unset($data['allowedRoleIds']);
        }
        elseif (\array_key_exists('allowedRoleIds', $data) && $data['allowedRoleIds'] === null) {
            $object->setAllowedRoleIds(null);
        }
        if (\array_key_exists('code', $data) && $data['code'] !== null) {
            $object->setCode($data['code']);
            unset($data['code']);
        }
        elseif (\array_key_exists('code', $data) && $data['code'] === null) {
            $object->setCode(null);
        }
        if (\array_key_exists('mappingScope', $data) && $data['mappingScope'] !== null) {
            $object->setMappingScope($data['mappingScope']);
            unset($data['mappingScope']);
        }
        elseif (\array_key_exists('mappingScope', $data) && $data['mappingScope'] === null) {
            $object->setMappingScope(null);
        }
        if (\array_key_exists('name', $data) && $data['name'] !== null) {
            $object->setName($data['name']);
            unset($data['name']);
        }
        elseif (\array_key_exists('name', $data) && $data['name'] === null) {
            $object->setName(null);
        }
        if (\array_key_exists('oidcClientId', $data) && $data['oidcClientId'] !== null) {
            $object->setOidcClientId($data['oidcClientId']);
            unset($data['oidcClientId']);
        }
        elseif (\array_key_exists('oidcClientId', $data) && $data['oidcClientId'] === null) {
            $object->setOidcClientId(null);
        }
        if (\array_key_exists('oidcClientSecretRef', $data) && $data['oidcClientSecretRef'] !== null) {
            $object->setOidcClientSecretRef($data['oidcClientSecretRef']);
            unset($data['oidcClientSecretRef']);
        }
        elseif (\array_key_exists('oidcClientSecretRef', $data) && $data['oidcClientSecretRef'] === null) {
            $object->setOidcClientSecretRef(null);
        }
        if (\array_key_exists('oidcIssuerPattern', $data) && $data['oidcIssuerPattern'] !== null) {
            $object->setOidcIssuerPattern($data['oidcIssuerPattern']);
            unset($data['oidcIssuerPattern']);
        }
        elseif (\array_key_exists('oidcIssuerPattern', $data) && $data['oidcIssuerPattern'] === null) {
            $object->setOidcIssuerPattern(null);
        }
        if (\array_key_exists('oidcIssuerUrl', $data) && $data['oidcIssuerUrl'] !== null) {
            $object->setOidcIssuerUrl($data['oidcIssuerUrl']);
            unset($data['oidcIssuerUrl']);
        }
        elseif (\array_key_exists('oidcIssuerUrl', $data) && $data['oidcIssuerUrl'] === null) {
            $object->setOidcIssuerUrl(null);
        }
        if (\array_key_exists('oidcMultiTenant', $data) && $data['oidcMultiTenant'] !== null) {
            $object->setOidcMultiTenant($data['oidcMultiTenant']);
            unset($data['oidcMultiTenant']);
        }
        elseif (\array_key_exists('oidcMultiTenant', $data) && $data['oidcMultiTenant'] === null) {
            $object->setOidcMultiTenant(null);
        }
        if (\array_key_exists('primaryClientId', $data) && $data['primaryClientId'] !== null) {
            $object->setPrimaryClientId($data['primaryClientId']);
            unset($data['primaryClientId']);
        }
        elseif (\array_key_exists('primaryClientId', $data) && $data['primaryClientId'] === null) {
            $object->setPrimaryClientId(null);
        }
        if (\array_key_exists('syncRolesFromIdp', $data) && $data['syncRolesFromIdp'] !== null) {
            $object->setSyncRolesFromIdp($data['syncRolesFromIdp']);
            unset($data['syncRolesFromIdp']);
        }
        elseif (\array_key_exists('syncRolesFromIdp', $data) && $data['syncRolesFromIdp'] === null) {
            $object->setSyncRolesFromIdp(null);
        }
        if (\array_key_exists('type', $data) && $data['type'] !== null) {
            $object->setType($data['type']);
            unset($data['type']);
        }
        elseif (\array_key_exists('type', $data) && $data['type'] === null) {
            $object->setType(null);
        }
        foreach ($data as $key => $value_2) {
            if (preg_match('/.*/', (string) $key)) {
                $object[$key] = $value_2;
            }
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        if ($data->isInitialized('allowedEmailDomains') && null !== $data->getAllowedEmailDomains()) {
            $values = [];
            foreach ($data->getAllowedEmailDomains() as $value) {
                $values[] = $value;
            }
            $dataArray['allowedEmailDomains'] = $values;
        }
        if ($data->isInitialized('allowedRoleIds') && null !== $data->getAllowedRoleIds()) {
            $values_1 = [];
            foreach ($data->getAllowedRoleIds() as $value_1) {
                $values_1[] = $value_1;
            }
            $dataArray['allowedRoleIds'] = $values_1;
        }
        $dataArray['code'] = $data->getCode();
        if ($data->isInitialized('mappingScope') && null !== $data->getMappingScope()) {
            $dataArray['mappingScope'] = $data->getMappingScope();
        }
        $dataArray['name'] = $data->getName();
        if ($data->isInitialized('oidcClientId') && null !== $data->getOidcClientId()) {
            $dataArray['oidcClientId'] = $data->getOidcClientId();
        }
        if ($data->isInitialized('oidcClientSecretRef') && null !== $data->getOidcClientSecretRef()) {
            $dataArray['oidcClientSecretRef'] = $data->getOidcClientSecretRef();
        }
        if ($data->isInitialized('oidcIssuerPattern') && null !== $data->getOidcIssuerPattern()) {
            $dataArray['oidcIssuerPattern'] = $data->getOidcIssuerPattern();
        }
        if ($data->isInitialized('oidcIssuerUrl') && null !== $data->getOidcIssuerUrl()) {
            $dataArray['oidcIssuerUrl'] = $data->getOidcIssuerUrl();
        }
        if ($data->isInitialized('oidcMultiTenant') && null !== $data->getOidcMultiTenant()) {
            $dataArray['oidcMultiTenant'] = $data->getOidcMultiTenant();
        }
        if ($data->isInitialized('primaryClientId') && null !== $data->getPrimaryClientId()) {
            $dataArray['primaryClientId'] = $data->getPrimaryClientId();
        }
        if ($data->isInitialized('syncRolesFromIdp') && null !== $data->getSyncRolesFromIdp()) {
            $dataArray['syncRolesFromIdp'] = $data->getSyncRolesFromIdp();
        }
        $dataArray['type'] = $data->getType();
        foreach ($data as $key => $value_2) {
            if (preg_match('/.*/', (string) $key)) {
                $dataArray[$key] = $value_2;
            }
        }
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\CreateIdentityProviderRequest::class => false];
    }
}