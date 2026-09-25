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
class CreatePortalAppResponseNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\CreatePortalAppResponse::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\CreatePortalAppResponse::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\CreatePortalAppResponse();
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
        if (\array_key_exists('clientSecret', $data) && $data['clientSecret'] !== null) {
            $object->setClientSecret($data['clientSecret']);
        }
        elseif (\array_key_exists('clientSecret', $data) && $data['clientSecret'] === null) {
            $object->setClientSecret(null);
        }
        if (\array_key_exists('clientType', $data) && $data['clientType'] !== null) {
            $object->setClientType($data['clientType']);
        }
        elseif (\array_key_exists('clientType', $data) && $data['clientType'] === null) {
            $object->setClientType(null);
        }
        if (\array_key_exists('oauthClientId', $data) && $data['oauthClientId'] !== null) {
            $object->setOauthClientId($data['oauthClientId']);
        }
        elseif (\array_key_exists('oauthClientId', $data) && $data['oauthClientId'] === null) {
            $object->setOauthClientId(null);
        }
        if (\array_key_exists('oauthClientRowId', $data) && $data['oauthClientRowId'] !== null) {
            $object->setOauthClientRowId($data['oauthClientRowId']);
        }
        elseif (\array_key_exists('oauthClientRowId', $data) && $data['oauthClientRowId'] === null) {
            $object->setOauthClientRowId(null);
        }
        if (\array_key_exists('portalApp', $data) && $data['portalApp'] !== null) {
            $object->setPortalApp($this->denormalizer->denormalize($data['portalApp'], \FlowCatalyst\Generated\Model\PortalAppResponse::class, 'json', $context));
        }
        elseif (\array_key_exists('portalApp', $data) && $data['portalApp'] === null) {
            $object->setPortalApp(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        if ($data->isInitialized('clientSecret') && null !== $data->getClientSecret()) {
            $dataArray['clientSecret'] = $data->getClientSecret();
        }
        $dataArray['clientType'] = $data->getClientType();
        $dataArray['oauthClientId'] = $data->getOauthClientId();
        $dataArray['oauthClientRowId'] = $data->getOauthClientRowId();
        $dataArray['portalApp'] = $this->normalizer->normalize($data->getPortalApp(), 'json', $context);
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\CreatePortalAppResponse::class => false];
    }
}