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
class CheckEmailDomainResponseNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\CheckEmailDomainResponse::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\CheckEmailDomainResponse::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\CheckEmailDomainResponse();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('emailExists', $data) && \is_int($data['emailExists'])) {
            $data['emailExists'] = (bool) $data['emailExists'];
        }
        if (\array_key_exists('hasIdpConfig', $data) && \is_int($data['hasIdpConfig'])) {
            $data['hasIdpConfig'] = (bool) $data['hasIdpConfig'];
        }
        if (\array_key_exists('isAnchorDomain', $data) && \is_int($data['isAnchorDomain'])) {
            $data['isAnchorDomain'] = (bool) $data['isAnchorDomain'];
        }
        if (\array_key_exists('requiresClientId', $data) && \is_int($data['requiresClientId'])) {
            $data['requiresClientId'] = (bool) $data['requiresClientId'];
        }
        if (\array_key_exists('$schema', $data) && $data['$schema'] !== null) {
            $object->setDollarSchema($data['$schema']);
        }
        elseif (\array_key_exists('$schema', $data) && $data['$schema'] === null) {
            $object->setDollarSchema(null);
        }
        if (\array_key_exists('allowedClientIds', $data) && $data['allowedClientIds'] !== null) {
            $values = [];
            foreach ($data['allowedClientIds'] as $value) {
                $values[] = $value;
            }
            $object->setAllowedClientIds($values);
        }
        elseif (\array_key_exists('allowedClientIds', $data) && $data['allowedClientIds'] === null) {
            $object->setAllowedClientIds(null);
        }
        if (\array_key_exists('authMethod', $data) && $data['authMethod'] !== null) {
            $object->setAuthMethod($data['authMethod']);
        }
        elseif (\array_key_exists('authMethod', $data) && $data['authMethod'] === null) {
            $object->setAuthMethod(null);
        }
        if (\array_key_exists('authProvider', $data) && $data['authProvider'] !== null) {
            $object->setAuthProvider($data['authProvider']);
        }
        elseif (\array_key_exists('authProvider', $data) && $data['authProvider'] === null) {
            $object->setAuthProvider(null);
        }
        if (\array_key_exists('derivedScope', $data) && $data['derivedScope'] !== null) {
            $object->setDerivedScope($data['derivedScope']);
        }
        elseif (\array_key_exists('derivedScope', $data) && $data['derivedScope'] === null) {
            $object->setDerivedScope(null);
        }
        if (\array_key_exists('domain', $data) && $data['domain'] !== null) {
            $object->setDomain($data['domain']);
        }
        elseif (\array_key_exists('domain', $data) && $data['domain'] === null) {
            $object->setDomain(null);
        }
        if (\array_key_exists('emailExists', $data) && $data['emailExists'] !== null) {
            $object->setEmailExists($data['emailExists']);
        }
        elseif (\array_key_exists('emailExists', $data) && $data['emailExists'] === null) {
            $object->setEmailExists(null);
        }
        if (\array_key_exists('hasIdpConfig', $data) && $data['hasIdpConfig'] !== null) {
            $object->setHasIdpConfig($data['hasIdpConfig']);
        }
        elseif (\array_key_exists('hasIdpConfig', $data) && $data['hasIdpConfig'] === null) {
            $object->setHasIdpConfig(null);
        }
        if (\array_key_exists('idpIssuer', $data) && $data['idpIssuer'] !== null) {
            $object->setIdpIssuer($data['idpIssuer']);
        }
        elseif (\array_key_exists('idpIssuer', $data) && $data['idpIssuer'] === null) {
            $object->setIdpIssuer(null);
        }
        if (\array_key_exists('info', $data) && $data['info'] !== null) {
            $object->setInfo($data['info']);
        }
        elseif (\array_key_exists('info', $data) && $data['info'] === null) {
            $object->setInfo(null);
        }
        if (\array_key_exists('isAnchorDomain', $data) && $data['isAnchorDomain'] !== null) {
            $object->setIsAnchorDomain($data['isAnchorDomain']);
        }
        elseif (\array_key_exists('isAnchorDomain', $data) && $data['isAnchorDomain'] === null) {
            $object->setIsAnchorDomain(null);
        }
        if (\array_key_exists('loginUrl', $data) && $data['loginUrl'] !== null) {
            $object->setLoginUrl($data['loginUrl']);
        }
        elseif (\array_key_exists('loginUrl', $data) && $data['loginUrl'] === null) {
            $object->setLoginUrl(null);
        }
        if (\array_key_exists('requiresClientId', $data) && $data['requiresClientId'] !== null) {
            $object->setRequiresClientId($data['requiresClientId']);
        }
        elseif (\array_key_exists('requiresClientId', $data) && $data['requiresClientId'] === null) {
            $object->setRequiresClientId(null);
        }
        if (\array_key_exists('warning', $data) && $data['warning'] !== null) {
            $object->setWarning($data['warning']);
        }
        elseif (\array_key_exists('warning', $data) && $data['warning'] === null) {
            $object->setWarning(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $values = [];
        foreach ($data->getAllowedClientIds() as $value) {
            $values[] = $value;
        }
        $dataArray['allowedClientIds'] = $values;
        $dataArray['authMethod'] = $data->getAuthMethod();
        $dataArray['authProvider'] = $data->getAuthProvider();
        $dataArray['derivedScope'] = $data->getDerivedScope();
        $dataArray['domain'] = $data->getDomain();
        $dataArray['emailExists'] = $data->getEmailExists();
        $dataArray['hasIdpConfig'] = $data->getHasIdpConfig();
        if ($data->isInitialized('idpIssuer') && null !== $data->getIdpIssuer()) {
            $dataArray['idpIssuer'] = $data->getIdpIssuer();
        }
        $dataArray['info'] = $data->getInfo();
        $dataArray['isAnchorDomain'] = $data->getIsAnchorDomain();
        if ($data->isInitialized('loginUrl') && null !== $data->getLoginUrl()) {
            $dataArray['loginUrl'] = $data->getLoginUrl();
        }
        $dataArray['requiresClientId'] = $data->getRequiresClientId();
        $dataArray['warning'] = $data->getWarning();
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\CheckEmailDomainResponse::class => false];
    }
}