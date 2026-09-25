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
class ApplicationLoginClientCredentialsNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\ApplicationLoginClientCredentials::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\ApplicationLoginClientCredentials::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\ApplicationLoginClientCredentials();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('clientType', $data) && $data['clientType'] !== null) {
            $object->setClientType($data['clientType']);
        }
        elseif (\array_key_exists('clientType', $data) && $data['clientType'] === null) {
            $object->setClientType(null);
        }
        if (\array_key_exists('oauthClient', $data) && $data['oauthClient'] !== null) {
            $object->setOauthClient($this->denormalizer->denormalize($data['oauthClient'], \FlowCatalyst\Generated\Model\ApplicationOAuthClientCredentials::class, 'json', $context));
        }
        elseif (\array_key_exists('oauthClient', $data) && $data['oauthClient'] === null) {
            $object->setOauthClient(null);
        }
        if (\array_key_exists('redirectUris', $data) && $data['redirectUris'] !== null) {
            $values = [];
            foreach ($data['redirectUris'] as $value) {
                $values[] = $value;
            }
            $object->setRedirectUris($values);
        }
        elseif (\array_key_exists('redirectUris', $data) && $data['redirectUris'] === null) {
            $object->setRedirectUris(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $dataArray['clientType'] = $data->getClientType();
        $dataArray['oauthClient'] = $this->normalizer->normalize($data->getOauthClient(), 'json', $context);
        $values = [];
        foreach ($data->getRedirectUris() as $value) {
            $values[] = $value;
        }
        $dataArray['redirectUris'] = $values;
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\ApplicationLoginClientCredentials::class => false];
    }
}