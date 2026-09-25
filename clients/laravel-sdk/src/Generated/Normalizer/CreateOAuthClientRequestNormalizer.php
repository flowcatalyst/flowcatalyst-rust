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
class CreateOAuthClientRequestNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\CreateOAuthClientRequest::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\CreateOAuthClientRequest::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\CreateOAuthClientRequest();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('apiAccess', $data) && \is_int($data['apiAccess'])) {
            $data['apiAccess'] = (bool) $data['apiAccess'];
        }
        if (\array_key_exists('pkceRequired', $data) && \is_int($data['pkceRequired'])) {
            $data['pkceRequired'] = (bool) $data['pkceRequired'];
        }
        if (\array_key_exists('$schema', $data) && $data['$schema'] !== null) {
            $object->setDollarSchema($data['$schema']);
            unset($data['$schema']);
        }
        elseif (\array_key_exists('$schema', $data) && $data['$schema'] === null) {
            $object->setDollarSchema(null);
        }
        if (\array_key_exists('allowedOrigins', $data) && $data['allowedOrigins'] !== null) {
            $values = [];
            foreach ($data['allowedOrigins'] as $value) {
                $values[] = $value;
            }
            $object->setAllowedOrigins($values);
            unset($data['allowedOrigins']);
        }
        elseif (\array_key_exists('allowedOrigins', $data) && $data['allowedOrigins'] === null) {
            $object->setAllowedOrigins(null);
        }
        if (\array_key_exists('apiAccess', $data) && $data['apiAccess'] !== null) {
            $object->setApiAccess($data['apiAccess']);
            unset($data['apiAccess']);
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
            unset($data['applicationIds']);
        }
        elseif (\array_key_exists('applicationIds', $data) && $data['applicationIds'] === null) {
            $object->setApplicationIds(null);
        }
        if (\array_key_exists('clientName', $data) && $data['clientName'] !== null) {
            $object->setClientName($data['clientName']);
            unset($data['clientName']);
        }
        elseif (\array_key_exists('clientName', $data) && $data['clientName'] === null) {
            $object->setClientName(null);
        }
        if (\array_key_exists('clientType', $data) && $data['clientType'] !== null) {
            $object->setClientType($data['clientType']);
            unset($data['clientType']);
        }
        elseif (\array_key_exists('clientType', $data) && $data['clientType'] === null) {
            $object->setClientType(null);
        }
        if (\array_key_exists('defaultScopes', $data) && $data['defaultScopes'] !== null) {
            $values_2 = [];
            foreach ($data['defaultScopes'] as $value_2) {
                $values_2[] = $value_2;
            }
            $object->setDefaultScopes($values_2);
            unset($data['defaultScopes']);
        }
        elseif (\array_key_exists('defaultScopes', $data) && $data['defaultScopes'] === null) {
            $object->setDefaultScopes(null);
        }
        if (\array_key_exists('grantTypes', $data) && $data['grantTypes'] !== null) {
            $values_3 = [];
            foreach ($data['grantTypes'] as $value_3) {
                $values_3[] = $value_3;
            }
            $object->setGrantTypes($values_3);
            unset($data['grantTypes']);
        }
        elseif (\array_key_exists('grantTypes', $data) && $data['grantTypes'] === null) {
            $object->setGrantTypes(null);
        }
        if (\array_key_exists('pkceRequired', $data) && $data['pkceRequired'] !== null) {
            $object->setPkceRequired($data['pkceRequired']);
            unset($data['pkceRequired']);
        }
        elseif (\array_key_exists('pkceRequired', $data) && $data['pkceRequired'] === null) {
            $object->setPkceRequired(null);
        }
        if (\array_key_exists('portalAppId', $data) && $data['portalAppId'] !== null) {
            $object->setPortalAppId($data['portalAppId']);
            unset($data['portalAppId']);
        }
        elseif (\array_key_exists('portalAppId', $data) && $data['portalAppId'] === null) {
            $object->setPortalAppId(null);
        }
        if (\array_key_exists('portalClientId', $data) && $data['portalClientId'] !== null) {
            $object->setPortalClientId($data['portalClientId']);
            unset($data['portalClientId']);
        }
        elseif (\array_key_exists('portalClientId', $data) && $data['portalClientId'] === null) {
            $object->setPortalClientId(null);
        }
        if (\array_key_exists('postLogoutRedirectUris', $data) && $data['postLogoutRedirectUris'] !== null) {
            $values_4 = [];
            foreach ($data['postLogoutRedirectUris'] as $value_4) {
                $values_4[] = $value_4;
            }
            $object->setPostLogoutRedirectUris($values_4);
            unset($data['postLogoutRedirectUris']);
        }
        elseif (\array_key_exists('postLogoutRedirectUris', $data) && $data['postLogoutRedirectUris'] === null) {
            $object->setPostLogoutRedirectUris(null);
        }
        if (\array_key_exists('principalId', $data) && $data['principalId'] !== null) {
            $object->setPrincipalId($data['principalId']);
            unset($data['principalId']);
        }
        elseif (\array_key_exists('principalId', $data) && $data['principalId'] === null) {
            $object->setPrincipalId(null);
        }
        if (\array_key_exists('redirectUris', $data) && $data['redirectUris'] !== null) {
            $values_5 = [];
            foreach ($data['redirectUris'] as $value_5) {
                $values_5[] = $value_5;
            }
            $object->setRedirectUris($values_5);
            unset($data['redirectUris']);
        }
        elseif (\array_key_exists('redirectUris', $data) && $data['redirectUris'] === null) {
            $object->setRedirectUris(null);
        }
        foreach ($data as $key => $value_6) {
            if (preg_match('/.*/', (string) $key)) {
                $object[$key] = $value_6;
            }
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        if ($data->isInitialized('allowedOrigins') && null !== $data->getAllowedOrigins()) {
            $values = [];
            foreach ($data->getAllowedOrigins() as $value) {
                $values[] = $value;
            }
            $dataArray['allowedOrigins'] = $values;
        }
        if ($data->isInitialized('apiAccess') && null !== $data->getApiAccess()) {
            $dataArray['apiAccess'] = $data->getApiAccess();
        }
        if ($data->isInitialized('applicationIds') && null !== $data->getApplicationIds()) {
            $values_1 = [];
            foreach ($data->getApplicationIds() as $value_1) {
                $values_1[] = $value_1;
            }
            $dataArray['applicationIds'] = $values_1;
        }
        $dataArray['clientName'] = $data->getClientName();
        $dataArray['clientType'] = $data->getClientType();
        if ($data->isInitialized('defaultScopes') && null !== $data->getDefaultScopes()) {
            $values_2 = [];
            foreach ($data->getDefaultScopes() as $value_2) {
                $values_2[] = $value_2;
            }
            $dataArray['defaultScopes'] = $values_2;
        }
        if ($data->isInitialized('grantTypes') && null !== $data->getGrantTypes()) {
            $values_3 = [];
            foreach ($data->getGrantTypes() as $value_3) {
                $values_3[] = $value_3;
            }
            $dataArray['grantTypes'] = $values_3;
        }
        if ($data->isInitialized('pkceRequired') && null !== $data->getPkceRequired()) {
            $dataArray['pkceRequired'] = $data->getPkceRequired();
        }
        if ($data->isInitialized('portalAppId') && null !== $data->getPortalAppId()) {
            $dataArray['portalAppId'] = $data->getPortalAppId();
        }
        if ($data->isInitialized('portalClientId') && null !== $data->getPortalClientId()) {
            $dataArray['portalClientId'] = $data->getPortalClientId();
        }
        if ($data->isInitialized('postLogoutRedirectUris') && null !== $data->getPostLogoutRedirectUris()) {
            $values_4 = [];
            foreach ($data->getPostLogoutRedirectUris() as $value_4) {
                $values_4[] = $value_4;
            }
            $dataArray['postLogoutRedirectUris'] = $values_4;
        }
        if ($data->isInitialized('principalId') && null !== $data->getPrincipalId()) {
            $dataArray['principalId'] = $data->getPrincipalId();
        }
        if ($data->isInitialized('redirectUris') && null !== $data->getRedirectUris()) {
            $values_5 = [];
            foreach ($data->getRedirectUris() as $value_5) {
                $values_5[] = $value_5;
            }
            $dataArray['redirectUris'] = $values_5;
        }
        foreach ($data as $key => $value_6) {
            if (preg_match('/.*/', (string) $key)) {
                $dataArray[$key] = $value_6;
            }
        }
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\CreateOAuthClientRequest::class => false];
    }
}