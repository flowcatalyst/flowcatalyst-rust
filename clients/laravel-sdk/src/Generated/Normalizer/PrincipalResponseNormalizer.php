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
class PrincipalResponseNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\PrincipalResponse::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\PrincipalResponse::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\PrincipalResponse();
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
        if (\array_key_exists('hasDeveloperCredential', $data) && \is_int($data['hasDeveloperCredential'])) {
            $data['hasDeveloperCredential'] = (bool) $data['hasDeveloperCredential'];
        }
        if (\array_key_exists('isAnchorUser', $data) && \is_int($data['isAnchorUser'])) {
            $data['isAnchorUser'] = (bool) $data['isAnchorUser'];
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
        if (\array_key_exists('clientId', $data) && $data['clientId'] !== null) {
            $object->setClientId($data['clientId']);
        }
        elseif (\array_key_exists('clientId', $data) && $data['clientId'] === null) {
            $object->setClientId(null);
        }
        if (\array_key_exists('createdAt', $data) && $data['createdAt'] !== null) {
            $object->setCreatedAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['createdAt']));
        }
        elseif (\array_key_exists('createdAt', $data) && $data['createdAt'] === null) {
            $object->setCreatedAt(null);
        }
        if (\array_key_exists('developerCredentialUpdatedAt', $data) && $data['developerCredentialUpdatedAt'] !== null) {
            $object->setDeveloperCredentialUpdatedAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['developerCredentialUpdatedAt']));
        }
        elseif (\array_key_exists('developerCredentialUpdatedAt', $data) && $data['developerCredentialUpdatedAt'] === null) {
            $object->setDeveloperCredentialUpdatedAt(null);
        }
        if (\array_key_exists('email', $data) && $data['email'] !== null) {
            $object->setEmail($data['email']);
        }
        elseif (\array_key_exists('email', $data) && $data['email'] === null) {
            $object->setEmail(null);
        }
        if (\array_key_exists('grantedClientIds', $data) && $data['grantedClientIds'] !== null) {
            $values = [];
            foreach ($data['grantedClientIds'] as $value) {
                $values[] = $value;
            }
            $object->setGrantedClientIds($values);
        }
        elseif (\array_key_exists('grantedClientIds', $data) && $data['grantedClientIds'] === null) {
            $object->setGrantedClientIds(null);
        }
        if (\array_key_exists('hasDeveloperCredential', $data) && $data['hasDeveloperCredential'] !== null) {
            $object->setHasDeveloperCredential($data['hasDeveloperCredential']);
        }
        elseif (\array_key_exists('hasDeveloperCredential', $data) && $data['hasDeveloperCredential'] === null) {
            $object->setHasDeveloperCredential(null);
        }
        if (\array_key_exists('id', $data) && $data['id'] !== null) {
            $object->setId($data['id']);
        }
        elseif (\array_key_exists('id', $data) && $data['id'] === null) {
            $object->setId(null);
        }
        if (\array_key_exists('idpType', $data) && $data['idpType'] !== null) {
            $object->setIdpType($data['idpType']);
        }
        elseif (\array_key_exists('idpType', $data) && $data['idpType'] === null) {
            $object->setIdpType(null);
        }
        if (\array_key_exists('inviteLink', $data) && $data['inviteLink'] !== null) {
            $object->setInviteLink($data['inviteLink']);
        }
        elseif (\array_key_exists('inviteLink', $data) && $data['inviteLink'] === null) {
            $object->setInviteLink(null);
        }
        if (\array_key_exists('isAnchorUser', $data) && $data['isAnchorUser'] !== null) {
            $object->setIsAnchorUser($data['isAnchorUser']);
        }
        elseif (\array_key_exists('isAnchorUser', $data) && $data['isAnchorUser'] === null) {
            $object->setIsAnchorUser(null);
        }
        if (\array_key_exists('name', $data) && $data['name'] !== null) {
            $object->setName($data['name']);
        }
        elseif (\array_key_exists('name', $data) && $data['name'] === null) {
            $object->setName(null);
        }
        if (\array_key_exists('roles', $data) && $data['roles'] !== null) {
            $values_1 = [];
            foreach ($data['roles'] as $value_1) {
                $values_1[] = $value_1;
            }
            $object->setRoles($values_1);
        }
        elseif (\array_key_exists('roles', $data) && $data['roles'] === null) {
            $object->setRoles(null);
        }
        if (\array_key_exists('scope', $data) && $data['scope'] !== null) {
            $object->setScope($data['scope']);
        }
        elseif (\array_key_exists('scope', $data) && $data['scope'] === null) {
            $object->setScope(null);
        }
        if (\array_key_exists('twoFactorMethods', $data) && $data['twoFactorMethods'] !== null) {
            $values_2 = [];
            foreach ($data['twoFactorMethods'] as $value_2) {
                $values_2[] = $value_2;
            }
            $object->setTwoFactorMethods($values_2);
        }
        elseif (\array_key_exists('twoFactorMethods', $data) && $data['twoFactorMethods'] === null) {
            $object->setTwoFactorMethods(null);
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
        $dataArray['active'] = $data->getActive();
        if ($data->isInitialized('clientId') && null !== $data->getClientId()) {
            $dataArray['clientId'] = $data->getClientId();
        }
        $dataArray['createdAt'] = $data->getCreatedAt()->format('Y-m-d\TH:i:sP');
        if ($data->isInitialized('developerCredentialUpdatedAt') && null !== $data->getDeveloperCredentialUpdatedAt()) {
            $dataArray['developerCredentialUpdatedAt'] = $data->getDeveloperCredentialUpdatedAt()->format('Y-m-d\TH:i:sP');
        }
        if ($data->isInitialized('email') && null !== $data->getEmail()) {
            $dataArray['email'] = $data->getEmail();
        }
        $values = [];
        foreach ($data->getGrantedClientIds() as $value) {
            $values[] = $value;
        }
        $dataArray['grantedClientIds'] = $values;
        $dataArray['hasDeveloperCredential'] = $data->getHasDeveloperCredential();
        $dataArray['id'] = $data->getId();
        if ($data->isInitialized('idpType') && null !== $data->getIdpType()) {
            $dataArray['idpType'] = $data->getIdpType();
        }
        if ($data->isInitialized('inviteLink') && null !== $data->getInviteLink()) {
            $dataArray['inviteLink'] = $data->getInviteLink();
        }
        $dataArray['isAnchorUser'] = $data->getIsAnchorUser();
        $dataArray['name'] = $data->getName();
        $values_1 = [];
        foreach ($data->getRoles() as $value_1) {
            $values_1[] = $value_1;
        }
        $dataArray['roles'] = $values_1;
        $dataArray['scope'] = $data->getScope();
        if ($data->isInitialized('twoFactorMethods') && null !== $data->getTwoFactorMethods()) {
            $values_2 = [];
            foreach ($data->getTwoFactorMethods() as $value_2) {
                $values_2[] = $value_2;
            }
            $dataArray['twoFactorMethods'] = $values_2;
        }
        $dataArray['type'] = $data->getType();
        $dataArray['updatedAt'] = $data->getUpdatedAt()->format('Y-m-d\TH:i:sP');
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\PrincipalResponse::class => false];
    }
}