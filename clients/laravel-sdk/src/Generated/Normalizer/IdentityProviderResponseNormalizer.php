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
class IdentityProviderResponseNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\IdentityProviderResponse::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\IdentityProviderResponse::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\IdentityProviderResponse();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('hasClientSecret', $data) && \is_int($data['hasClientSecret'])) {
            $data['hasClientSecret'] = (bool) $data['hasClientSecret'];
        }
        if (\array_key_exists('oidcMultiTenant', $data) && \is_int($data['oidcMultiTenant'])) {
            $data['oidcMultiTenant'] = (bool) $data['oidcMultiTenant'];
        }
        if (\array_key_exists('syncRolesFromIdp', $data) && \is_int($data['syncRolesFromIdp'])) {
            $data['syncRolesFromIdp'] = (bool) $data['syncRolesFromIdp'];
        }
        if (\array_key_exists('$schema', $data) && $data['$schema'] !== null) {
            $object->setDollarSchema($data['$schema']);
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
        }
        elseif (\array_key_exists('allowedRoleIds', $data) && $data['allowedRoleIds'] === null) {
            $object->setAllowedRoleIds(null);
        }
        if (\array_key_exists('code', $data) && $data['code'] !== null) {
            $object->setCode($data['code']);
        }
        elseif (\array_key_exists('code', $data) && $data['code'] === null) {
            $object->setCode(null);
        }
        if (\array_key_exists('createdAt', $data) && $data['createdAt'] !== null) {
            $object->setCreatedAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['createdAt']));
        }
        elseif (\array_key_exists('createdAt', $data) && $data['createdAt'] === null) {
            $object->setCreatedAt(null);
        }
        if (\array_key_exists('hasClientSecret', $data) && $data['hasClientSecret'] !== null) {
            $object->setHasClientSecret($data['hasClientSecret']);
        }
        elseif (\array_key_exists('hasClientSecret', $data) && $data['hasClientSecret'] === null) {
            $object->setHasClientSecret(null);
        }
        if (\array_key_exists('id', $data) && $data['id'] !== null) {
            $object->setId($data['id']);
        }
        elseif (\array_key_exists('id', $data) && $data['id'] === null) {
            $object->setId(null);
        }
        if (\array_key_exists('name', $data) && $data['name'] !== null) {
            $object->setName($data['name']);
        }
        elseif (\array_key_exists('name', $data) && $data['name'] === null) {
            $object->setName(null);
        }
        if (\array_key_exists('oidcClientId', $data) && $data['oidcClientId'] !== null) {
            $object->setOidcClientId($data['oidcClientId']);
        }
        elseif (\array_key_exists('oidcClientId', $data) && $data['oidcClientId'] === null) {
            $object->setOidcClientId(null);
        }
        if (\array_key_exists('oidcIssuerPattern', $data) && $data['oidcIssuerPattern'] !== null) {
            $object->setOidcIssuerPattern($data['oidcIssuerPattern']);
        }
        elseif (\array_key_exists('oidcIssuerPattern', $data) && $data['oidcIssuerPattern'] === null) {
            $object->setOidcIssuerPattern(null);
        }
        if (\array_key_exists('oidcIssuerUrl', $data) && $data['oidcIssuerUrl'] !== null) {
            $object->setOidcIssuerUrl($data['oidcIssuerUrl']);
        }
        elseif (\array_key_exists('oidcIssuerUrl', $data) && $data['oidcIssuerUrl'] === null) {
            $object->setOidcIssuerUrl(null);
        }
        if (\array_key_exists('oidcMultiTenant', $data) && $data['oidcMultiTenant'] !== null) {
            $object->setOidcMultiTenant($data['oidcMultiTenant']);
        }
        elseif (\array_key_exists('oidcMultiTenant', $data) && $data['oidcMultiTenant'] === null) {
            $object->setOidcMultiTenant(null);
        }
        if (\array_key_exists('syncRolesFromIdp', $data) && $data['syncRolesFromIdp'] !== null) {
            $object->setSyncRolesFromIdp($data['syncRolesFromIdp']);
        }
        elseif (\array_key_exists('syncRolesFromIdp', $data) && $data['syncRolesFromIdp'] === null) {
            $object->setSyncRolesFromIdp(null);
        }
        if (\array_key_exists('type', $data) && $data['type'] !== null) {
            $object->setType($data['type']);
        }
        elseif (\array_key_exists('type', $data) && $data['type'] === null) {
            $object->setType(null);
        }
        if (\array_key_exists('updatedAt', $data) && $data['updatedAt'] !== null) {
            $object->setUpdatedAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['updatedAt']));
        }
        elseif (\array_key_exists('updatedAt', $data) && $data['updatedAt'] === null) {
            $object->setUpdatedAt(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $values = [];
        foreach ($data->getAllowedEmailDomains() as $value) {
            $values[] = $value;
        }
        $dataArray['allowedEmailDomains'] = $values;
        $values_1 = [];
        foreach ($data->getAllowedRoleIds() as $value_1) {
            $values_1[] = $value_1;
        }
        $dataArray['allowedRoleIds'] = $values_1;
        $dataArray['code'] = $data->getCode();
        $dataArray['createdAt'] = $data->getCreatedAt()->format('Y-m-d\TH:i:sP');
        $dataArray['hasClientSecret'] = $data->getHasClientSecret();
        $dataArray['id'] = $data->getId();
        $dataArray['name'] = $data->getName();
        if ($data->isInitialized('oidcClientId') && null !== $data->getOidcClientId()) {
            $dataArray['oidcClientId'] = $data->getOidcClientId();
        }
        if ($data->isInitialized('oidcIssuerPattern') && null !== $data->getOidcIssuerPattern()) {
            $dataArray['oidcIssuerPattern'] = $data->getOidcIssuerPattern();
        }
        if ($data->isInitialized('oidcIssuerUrl') && null !== $data->getOidcIssuerUrl()) {
            $dataArray['oidcIssuerUrl'] = $data->getOidcIssuerUrl();
        }
        $dataArray['oidcMultiTenant'] = $data->getOidcMultiTenant();
        $dataArray['syncRolesFromIdp'] = $data->getSyncRolesFromIdp();
        $dataArray['type'] = $data->getType();
        $dataArray['updatedAt'] = $data->getUpdatedAt()->format('Y-m-d\TH:i:sP');
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\IdentityProviderResponse::class => false];
    }
}