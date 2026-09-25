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
class OAuthClientResponseNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\OAuthClientResponse::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\OAuthClientResponse::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\OAuthClientResponse();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('active', $data) && \is_int($data['active'])) {
            $data['active'] = (bool) $data['active'];
        }
        if (\array_key_exists('apiAccess', $data) && \is_int($data['apiAccess'])) {
            $data['apiAccess'] = (bool) $data['apiAccess'];
        }
        if (\array_key_exists('pkceRequired', $data) && \is_int($data['pkceRequired'])) {
            $data['pkceRequired'] = (bool) $data['pkceRequired'];
        }
        if (\array_key_exists('$schema', $data) && $data['$schema'] !== null) {
            $object->setDollarSchema($data['$schema']);
        }
        elseif (\array_key_exists('$schema', $data) && $data['$schema'] === null) {
            $object->setDollarSchema(null);
        }
        if (\array_key_exists('active', $data) && $data['active'] !== null) {
            $object->setActive($data['active']);
        }
        elseif (\array_key_exists('active', $data) && $data['active'] === null) {
            $object->setActive(null);
        }
        if (\array_key_exists('allowedOrigins', $data) && $data['allowedOrigins'] !== null) {
            $values = [];
            foreach ($data['allowedOrigins'] as $value) {
                $values[] = $value;
            }
            $object->setAllowedOrigins($values);
        }
        elseif (\array_key_exists('allowedOrigins', $data) && $data['allowedOrigins'] === null) {
            $object->setAllowedOrigins(null);
        }
        if (\array_key_exists('apiAccess', $data) && $data['apiAccess'] !== null) {
            $object->setApiAccess($data['apiAccess']);
        }
        elseif (\array_key_exists('apiAccess', $data) && $data['apiAccess'] === null) {
            $object->setApiAccess(null);
        }
        if (\array_key_exists('applicationIds', $data) && $data['applicationIds'] !== null) {
            $values_1 = [];
            foreach ($data['applicationIds'] as $value_1) {
                $values_1[] = $value_1;
            }
            $object->setApplicationIds($values_1);
        }
        elseif (\array_key_exists('applicationIds', $data) && $data['applicationIds'] === null) {
            $object->setApplicationIds(null);
        }
        if (\array_key_exists('applications', $data) && $data['applications'] !== null) {
            $values_2 = [];
            foreach ($data['applications'] as $value_2) {
                $values_2[] = $this->denormalizer->denormalize($value_2, \FlowCatalyst\Generated\Model\OAuthClientApplicationRef::class, 'json', $context);
            }
            $object->setApplications($values_2);
        }
        elseif (\array_key_exists('applications', $data) && $data['applications'] === null) {
            $object->setApplications(null);
        }
        if (\array_key_exists('clientId', $data) && $data['clientId'] !== null) {
            $object->setClientId($data['clientId']);
        }
        elseif (\array_key_exists('clientId', $data) && $data['clientId'] === null) {
            $object->setClientId(null);
        }
        if (\array_key_exists('clientName', $data) && $data['clientName'] !== null) {
            $object->setClientName($data['clientName']);
        }
        elseif (\array_key_exists('clientName', $data) && $data['clientName'] === null) {
            $object->setClientName(null);
        }
        if (\array_key_exists('clientType', $data) && $data['clientType'] !== null) {
            $object->setClientType($data['clientType']);
        }
        elseif (\array_key_exists('clientType', $data) && $data['clientType'] === null) {
            $object->setClientType(null);
        }
        if (\array_key_exists('createdAt', $data) && $data['createdAt'] !== null) {
            $object->setCreatedAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['createdAt']));
        }
        elseif (\array_key_exists('createdAt', $data) && $data['createdAt'] === null) {
            $object->setCreatedAt(null);
        }
        if (\array_key_exists('defaultScopes', $data) && $data['defaultScopes'] !== null) {
            $values_3 = [];
            foreach ($data['defaultScopes'] as $value_3) {
                $values_3[] = $value_3;
            }
            $object->setDefaultScopes($values_3);
        }
        elseif (\array_key_exists('defaultScopes', $data) && $data['defaultScopes'] === null) {
            $object->setDefaultScopes(null);
        }
        if (\array_key_exists('grantTypes', $data) && $data['grantTypes'] !== null) {
            $values_4 = [];
            foreach ($data['grantTypes'] as $value_4) {
                $values_4[] = $value_4;
            }
            $object->setGrantTypes($values_4);
        }
        elseif (\array_key_exists('grantTypes', $data) && $data['grantTypes'] === null) {
            $object->setGrantTypes(null);
        }
        if (\array_key_exists('id', $data) && $data['id'] !== null) {
            $object->setId($data['id']);
        }
        elseif (\array_key_exists('id', $data) && $data['id'] === null) {
            $object->setId(null);
        }
        if (\array_key_exists('pkceRequired', $data) && $data['pkceRequired'] !== null) {
            $object->setPkceRequired($data['pkceRequired']);
        }
        elseif (\array_key_exists('pkceRequired', $data) && $data['pkceRequired'] === null) {
            $object->setPkceRequired(null);
        }
        if (\array_key_exists('portalAppId', $data) && $data['portalAppId'] !== null) {
            $object->setPortalAppId($data['portalAppId']);
        }
        elseif (\array_key_exists('portalAppId', $data) && $data['portalAppId'] === null) {
            $object->setPortalAppId(null);
        }
        if (\array_key_exists('portalClientId', $data) && $data['portalClientId'] !== null) {
            $object->setPortalClientId($data['portalClientId']);
        }
        elseif (\array_key_exists('portalClientId', $data) && $data['portalClientId'] === null) {
            $object->setPortalClientId(null);
        }
        if (\array_key_exists('postLogoutRedirectUris', $data) && $data['postLogoutRedirectUris'] !== null) {
            $values_5 = [];
            foreach ($data['postLogoutRedirectUris'] as $value_5) {
                $values_5[] = $value_5;
            }
            $object->setPostLogoutRedirectUris($values_5);
        }
        elseif (\array_key_exists('postLogoutRedirectUris', $data) && $data['postLogoutRedirectUris'] === null) {
            $object->setPostLogoutRedirectUris(null);
        }
        if (\array_key_exists('previousSecretExpiresAt', $data) && $data['previousSecretExpiresAt'] !== null) {
            $object->setPreviousSecretExpiresAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['previousSecretExpiresAt']));
        }
        elseif (\array_key_exists('previousSecretExpiresAt', $data) && $data['previousSecretExpiresAt'] === null) {
            $object->setPreviousSecretExpiresAt(null);
        }
        if (\array_key_exists('previousSecretLastUsedAt', $data) && $data['previousSecretLastUsedAt'] !== null) {
            $object->setPreviousSecretLastUsedAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['previousSecretLastUsedAt']));
        }
        elseif (\array_key_exists('previousSecretLastUsedAt', $data) && $data['previousSecretLastUsedAt'] === null) {
            $object->setPreviousSecretLastUsedAt(null);
        }
        if (\array_key_exists('redirectUris', $data) && $data['redirectUris'] !== null) {
            $values_6 = [];
            foreach ($data['redirectUris'] as $value_6) {
                $values_6[] = $value_6;
            }
            $object->setRedirectUris($values_6);
        }
        elseif (\array_key_exists('redirectUris', $data) && $data['redirectUris'] === null) {
            $object->setRedirectUris(null);
        }
        if (\array_key_exists('serviceAccountPrincipalId', $data) && $data['serviceAccountPrincipalId'] !== null) {
            $object->setServiceAccountPrincipalId($data['serviceAccountPrincipalId']);
        }
        elseif (\array_key_exists('serviceAccountPrincipalId', $data) && $data['serviceAccountPrincipalId'] === null) {
            $object->setServiceAccountPrincipalId(null);
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
        $dataArray['active'] = $data->getActive();
        $values = [];
        foreach ($data->getAllowedOrigins() as $value) {
            $values[] = $value;
        }
        $dataArray['allowedOrigins'] = $values;
        $dataArray['apiAccess'] = $data->getApiAccess();
        $values_1 = [];
        foreach ($data->getApplicationIds() as $value_1) {
            $values_1[] = $value_1;
        }
        $dataArray['applicationIds'] = $values_1;
        $values_2 = [];
        foreach ($data->getApplications() as $value_2) {
            $values_2[] = $this->normalizer->normalize($value_2, 'json', $context);
        }
        $dataArray['applications'] = $values_2;
        $dataArray['clientId'] = $data->getClientId();
        $dataArray['clientName'] = $data->getClientName();
        $dataArray['clientType'] = $data->getClientType();
        $dataArray['createdAt'] = $data->getCreatedAt()->format('Y-m-d\TH:i:sP');
        $values_3 = [];
        foreach ($data->getDefaultScopes() as $value_3) {
            $values_3[] = $value_3;
        }
        $dataArray['defaultScopes'] = $values_3;
        $values_4 = [];
        foreach ($data->getGrantTypes() as $value_4) {
            $values_4[] = $value_4;
        }
        $dataArray['grantTypes'] = $values_4;
        $dataArray['id'] = $data->getId();
        $dataArray['pkceRequired'] = $data->getPkceRequired();
        if ($data->isInitialized('portalAppId') && null !== $data->getPortalAppId()) {
            $dataArray['portalAppId'] = $data->getPortalAppId();
        }
        if ($data->isInitialized('portalClientId') && null !== $data->getPortalClientId()) {
            $dataArray['portalClientId'] = $data->getPortalClientId();
        }
        $values_5 = [];
        foreach ($data->getPostLogoutRedirectUris() as $value_5) {
            $values_5[] = $value_5;
        }
        $dataArray['postLogoutRedirectUris'] = $values_5;
        if ($data->isInitialized('previousSecretExpiresAt') && null !== $data->getPreviousSecretExpiresAt()) {
            $dataArray['previousSecretExpiresAt'] = $data->getPreviousSecretExpiresAt()->format('Y-m-d\TH:i:sP');
        }
        if ($data->isInitialized('previousSecretLastUsedAt') && null !== $data->getPreviousSecretLastUsedAt()) {
            $dataArray['previousSecretLastUsedAt'] = $data->getPreviousSecretLastUsedAt()->format('Y-m-d\TH:i:sP');
        }
        $values_6 = [];
        foreach ($data->getRedirectUris() as $value_6) {
            $values_6[] = $value_6;
        }
        $dataArray['redirectUris'] = $values_6;
        if ($data->isInitialized('serviceAccountPrincipalId') && null !== $data->getServiceAccountPrincipalId()) {
            $dataArray['serviceAccountPrincipalId'] = $data->getServiceAccountPrincipalId();
        }
        $dataArray['updatedAt'] = $data->getUpdatedAt()->format('Y-m-d\TH:i:sP');
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\OAuthClientResponse::class => false];
    }
}