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
class PortalUserListItemNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\PortalUserListItem::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\PortalUserListItem::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\PortalUserListItem();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('hasPassword', $data) && \is_int($data['hasPassword'])) {
            $data['hasPassword'] = (bool) $data['hasPassword'];
        }
        if (\array_key_exists('apps', $data) && $data['apps'] !== null) {
            $values = [];
            foreach ($data['apps'] as $value) {
                $values[] = $this->denormalizer->denormalize($value, \FlowCatalyst\Generated\Model\PortalUserAppRef::class, 'json', $context);
            }
            $object->setApps($values);
        }
        elseif (\array_key_exists('apps', $data) && $data['apps'] === null) {
            $object->setApps(null);
        }
        if (\array_key_exists('createdAt', $data) && $data['createdAt'] !== null) {
            $object->setCreatedAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['createdAt']));
        }
        elseif (\array_key_exists('createdAt', $data) && $data['createdAt'] === null) {
            $object->setCreatedAt(null);
        }
        if (\array_key_exists('email', $data) && $data['email'] !== null) {
            $object->setEmail($data['email']);
        }
        elseif (\array_key_exists('email', $data) && $data['email'] === null) {
            $object->setEmail(null);
        }
        if (\array_key_exists('hasPassword', $data) && $data['hasPassword'] !== null) {
            $object->setHasPassword($data['hasPassword']);
        }
        elseif (\array_key_exists('hasPassword', $data) && $data['hasPassword'] === null) {
            $object->setHasPassword(null);
        }
        if (\array_key_exists('identityId', $data) && $data['identityId'] !== null) {
            $object->setIdentityId($data['identityId']);
        }
        elseif (\array_key_exists('identityId', $data) && $data['identityId'] === null) {
            $object->setIdentityId(null);
        }
        if (\array_key_exists('inviteExpiresAt', $data) && $data['inviteExpiresAt'] !== null) {
            $object->setInviteExpiresAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['inviteExpiresAt']));
        }
        elseif (\array_key_exists('inviteExpiresAt', $data) && $data['inviteExpiresAt'] === null) {
            $object->setInviteExpiresAt(null);
        }
        if (\array_key_exists('invitedAt', $data) && $data['invitedAt'] !== null) {
            $object->setInvitedAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['invitedAt']));
        }
        elseif (\array_key_exists('invitedAt', $data) && $data['invitedAt'] === null) {
            $object->setInvitedAt(null);
        }
        if (\array_key_exists('lastLoginAt', $data) && $data['lastLoginAt'] !== null) {
            $object->setLastLoginAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['lastLoginAt']));
        }
        elseif (\array_key_exists('lastLoginAt', $data) && $data['lastLoginAt'] === null) {
            $object->setLastLoginAt(null);
        }
        if (\array_key_exists('name', $data) && $data['name'] !== null) {
            $object->setName($data['name']);
        }
        elseif (\array_key_exists('name', $data) && $data['name'] === null) {
            $object->setName(null);
        }
        if (\array_key_exists('source', $data) && $data['source'] !== null) {
            $object->setSource($data['source']);
        }
        elseif (\array_key_exists('source', $data) && $data['source'] === null) {
            $object->setSource(null);
        }
        if (\array_key_exists('state', $data) && $data['state'] !== null) {
            $object->setState($data['state']);
        }
        elseif (\array_key_exists('state', $data) && $data['state'] === null) {
            $object->setState(null);
        }
        if (\array_key_exists('status', $data) && $data['status'] !== null) {
            $object->setStatus($data['status']);
        }
        elseif (\array_key_exists('status', $data) && $data['status'] === null) {
            $object->setStatus(null);
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
        foreach ($data->getApps() as $value) {
            $values[] = $this->normalizer->normalize($value, 'json', $context);
        }
        $dataArray['apps'] = $values;
        $dataArray['createdAt'] = $data->getCreatedAt()->format('Y-m-d\TH:i:sP');
        $dataArray['email'] = $data->getEmail();
        $dataArray['hasPassword'] = $data->getHasPassword();
        $dataArray['identityId'] = $data->getIdentityId();
        if ($data->isInitialized('inviteExpiresAt') && null !== $data->getInviteExpiresAt()) {
            $dataArray['inviteExpiresAt'] = $data->getInviteExpiresAt()->format('Y-m-d\TH:i:sP');
        }
        if ($data->isInitialized('invitedAt') && null !== $data->getInvitedAt()) {
            $dataArray['invitedAt'] = $data->getInvitedAt()->format('Y-m-d\TH:i:sP');
        }
        if ($data->isInitialized('lastLoginAt') && null !== $data->getLastLoginAt()) {
            $dataArray['lastLoginAt'] = $data->getLastLoginAt()->format('Y-m-d\TH:i:sP');
        }
        $dataArray['name'] = $data->getName();
        $dataArray['source'] = $data->getSource();
        $dataArray['state'] = $data->getState();
        $dataArray['status'] = $data->getStatus();
        $dataArray['updatedAt'] = $data->getUpdatedAt()->format('Y-m-d\TH:i:sP');
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\PortalUserListItem::class => false];
    }
}