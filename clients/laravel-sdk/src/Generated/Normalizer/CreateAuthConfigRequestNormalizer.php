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
class CreateAuthConfigRequestNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\CreateAuthConfigRequest::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\CreateAuthConfigRequest::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\CreateAuthConfigRequest();
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
        if (\array_key_exists('authProvider', $data) && $data['authProvider'] !== null) {
            $object->setAuthProvider($data['authProvider']);
            unset($data['authProvider']);
        }
        elseif (\array_key_exists('authProvider', $data) && $data['authProvider'] === null) {
            $object->setAuthProvider(null);
        }
        if (\array_key_exists('configType', $data) && $data['configType'] !== null) {
            $object->setConfigType($data['configType']);
            unset($data['configType']);
        }
        elseif (\array_key_exists('configType', $data) && $data['configType'] === null) {
            $object->setConfigType(null);
        }
        if (\array_key_exists('emailDomain', $data) && $data['emailDomain'] !== null) {
            $object->setEmailDomain($data['emailDomain']);
            unset($data['emailDomain']);
        }
        elseif (\array_key_exists('emailDomain', $data) && $data['emailDomain'] === null) {
            $object->setEmailDomain(null);
        }
        if (\array_key_exists('grantedClientIds', $data) && $data['grantedClientIds'] !== null) {
            $values_1 = [];
            foreach ($data['grantedClientIds'] as $value_1) {
                $values_1[] = $value_1;
            }
            $object->setGrantedClientIds($values_1);
            unset($data['grantedClientIds']);
        }
        elseif (\array_key_exists('grantedClientIds', $data) && $data['grantedClientIds'] === null) {
            $object->setGrantedClientIds(null);
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
        if ($data->isInitialized('additionalClientIds') && null !== $data->getAdditionalClientIds()) {
            $values = [];
            foreach ($data->getAdditionalClientIds() as $value) {
                $values[] = $value;
            }
            $dataArray['additionalClientIds'] = $values;
        }
        $dataArray['authProvider'] = $data->getAuthProvider();
        $dataArray['configType'] = $data->getConfigType();
        $dataArray['emailDomain'] = $data->getEmailDomain();
        if ($data->isInitialized('grantedClientIds') && null !== $data->getGrantedClientIds()) {
            $values_1 = [];
            foreach ($data->getGrantedClientIds() as $value_1) {
                $values_1[] = $value_1;
            }
            $dataArray['grantedClientIds'] = $values_1;
        }
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
        $dataArray['oidcMultiTenant'] = $data->getOidcMultiTenant();
        if ($data->isInitialized('primaryClientId') && null !== $data->getPrimaryClientId()) {
            $dataArray['primaryClientId'] = $data->getPrimaryClientId();
        }
        foreach ($data as $key => $value_2) {
            if (preg_match('/.*/', (string) $key)) {
                $dataArray[$key] = $value_2;
            }
        }
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\CreateAuthConfigRequest::class => false];
    }
}