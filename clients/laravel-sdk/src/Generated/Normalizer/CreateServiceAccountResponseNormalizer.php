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
class CreateServiceAccountResponseNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\CreateServiceAccountResponse::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\CreateServiceAccountResponse::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\CreateServiceAccountResponse();
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
        if (\array_key_exists('oauth', $data) && $data['oauth'] !== null) {
            $object->setOauth($this->denormalizer->denormalize($data['oauth'], \FlowCatalyst\Generated\Model\ServiceAccountOAuthSecrets::class, 'json', $context));
        }
        elseif (\array_key_exists('oauth', $data) && $data['oauth'] === null) {
            $object->setOauth(null);
        }
        if (\array_key_exists('principalId', $data) && $data['principalId'] !== null) {
            $object->setPrincipalId($data['principalId']);
        }
        elseif (\array_key_exists('principalId', $data) && $data['principalId'] === null) {
            $object->setPrincipalId(null);
        }
        if (\array_key_exists('serviceAccount', $data) && $data['serviceAccount'] !== null) {
            $object->setServiceAccount($this->denormalizer->denormalize($data['serviceAccount'], \FlowCatalyst\Generated\Model\ServiceAccountResponse::class, 'json', $context));
        }
        elseif (\array_key_exists('serviceAccount', $data) && $data['serviceAccount'] === null) {
            $object->setServiceAccount(null);
        }
        if (\array_key_exists('webhook', $data) && $data['webhook'] !== null) {
            $object->setWebhook($this->denormalizer->denormalize($data['webhook'], \FlowCatalyst\Generated\Model\ServiceAccountWebhookSecrets::class, 'json', $context));
        }
        elseif (\array_key_exists('webhook', $data) && $data['webhook'] === null) {
            $object->setWebhook(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $dataArray['oauth'] = $this->normalizer->normalize($data->getOauth(), 'json', $context);
        $dataArray['principalId'] = $data->getPrincipalId();
        $dataArray['serviceAccount'] = $this->normalizer->normalize($data->getServiceAccount(), 'json', $context);
        $dataArray['webhook'] = $this->normalizer->normalize($data->getWebhook(), 'json', $context);
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\CreateServiceAccountResponse::class => false];
    }
}