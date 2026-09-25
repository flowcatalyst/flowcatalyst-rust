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
class WebhookCredentialsDTONormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\WebhookCredentialsDTO::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\WebhookCredentialsDTO::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\WebhookCredentialsDTO();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('authType', $data) && $data['authType'] !== null) {
            $object->setAuthType($data['authType']);
        }
        elseif (\array_key_exists('authType', $data) && $data['authType'] === null) {
            $object->setAuthType(null);
        }
        if (\array_key_exists('headerName', $data) && $data['headerName'] !== null) {
            $object->setHeaderName($data['headerName']);
        }
        elseif (\array_key_exists('headerName', $data) && $data['headerName'] === null) {
            $object->setHeaderName(null);
        }
        if (\array_key_exists('password', $data) && $data['password'] !== null) {
            $object->setPassword($data['password']);
        }
        elseif (\array_key_exists('password', $data) && $data['password'] === null) {
            $object->setPassword(null);
        }
        if (\array_key_exists('signatureHeader', $data) && $data['signatureHeader'] !== null) {
            $object->setSignatureHeader($data['signatureHeader']);
        }
        elseif (\array_key_exists('signatureHeader', $data) && $data['signatureHeader'] === null) {
            $object->setSignatureHeader(null);
        }
        if (\array_key_exists('signingAlgorithm', $data) && $data['signingAlgorithm'] !== null) {
            $object->setSigningAlgorithm($data['signingAlgorithm']);
        }
        elseif (\array_key_exists('signingAlgorithm', $data) && $data['signingAlgorithm'] === null) {
            $object->setSigningAlgorithm(null);
        }
        if (\array_key_exists('signingSecret', $data) && $data['signingSecret'] !== null) {
            $object->setSigningSecret($data['signingSecret']);
        }
        elseif (\array_key_exists('signingSecret', $data) && $data['signingSecret'] === null) {
            $object->setSigningSecret(null);
        }
        if (\array_key_exists('token', $data) && $data['token'] !== null) {
            $object->setToken($data['token']);
        }
        elseif (\array_key_exists('token', $data) && $data['token'] === null) {
            $object->setToken(null);
        }
        if (\array_key_exists('username', $data) && $data['username'] !== null) {
            $object->setUsername($data['username']);
        }
        elseif (\array_key_exists('username', $data) && $data['username'] === null) {
            $object->setUsername(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $dataArray['authType'] = $data->getAuthType();
        if ($data->isInitialized('headerName') && null !== $data->getHeaderName()) {
            $dataArray['headerName'] = $data->getHeaderName();
        }
        if ($data->isInitialized('password') && null !== $data->getPassword()) {
            $dataArray['password'] = $data->getPassword();
        }
        if ($data->isInitialized('signatureHeader') && null !== $data->getSignatureHeader()) {
            $dataArray['signatureHeader'] = $data->getSignatureHeader();
        }
        if ($data->isInitialized('signingAlgorithm') && null !== $data->getSigningAlgorithm()) {
            $dataArray['signingAlgorithm'] = $data->getSigningAlgorithm();
        }
        if ($data->isInitialized('signingSecret') && null !== $data->getSigningSecret()) {
            $dataArray['signingSecret'] = $data->getSigningSecret();
        }
        if ($data->isInitialized('token') && null !== $data->getToken()) {
            $dataArray['token'] = $data->getToken();
        }
        if ($data->isInitialized('username') && null !== $data->getUsername()) {
            $dataArray['username'] = $data->getUsername();
        }
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\WebhookCredentialsDTO::class => false];
    }
}