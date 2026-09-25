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
class LoginAttemptResponseNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\LoginAttemptResponse::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\LoginAttemptResponse::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\LoginAttemptResponse();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('attemptType', $data) && $data['attemptType'] !== null) {
            $object->setAttemptType($data['attemptType']);
        }
        elseif (\array_key_exists('attemptType', $data) && $data['attemptType'] === null) {
            $object->setAttemptType(null);
        }
        if (\array_key_exists('attemptedAt', $data) && $data['attemptedAt'] !== null) {
            $object->setAttemptedAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['attemptedAt']));
        }
        elseif (\array_key_exists('attemptedAt', $data) && $data['attemptedAt'] === null) {
            $object->setAttemptedAt(null);
        }
        if (\array_key_exists('failureReason', $data) && $data['failureReason'] !== null) {
            $object->setFailureReason($data['failureReason']);
        }
        elseif (\array_key_exists('failureReason', $data) && $data['failureReason'] === null) {
            $object->setFailureReason(null);
        }
        if (\array_key_exists('id', $data) && $data['id'] !== null) {
            $object->setId($data['id']);
        }
        elseif (\array_key_exists('id', $data) && $data['id'] === null) {
            $object->setId(null);
        }
        if (\array_key_exists('identifier', $data) && $data['identifier'] !== null) {
            $object->setIdentifier($data['identifier']);
        }
        elseif (\array_key_exists('identifier', $data) && $data['identifier'] === null) {
            $object->setIdentifier(null);
        }
        if (\array_key_exists('ipAddress', $data) && $data['ipAddress'] !== null) {
            $object->setIpAddress($data['ipAddress']);
        }
        elseif (\array_key_exists('ipAddress', $data) && $data['ipAddress'] === null) {
            $object->setIpAddress(null);
        }
        if (\array_key_exists('outcome', $data) && $data['outcome'] !== null) {
            $object->setOutcome($data['outcome']);
        }
        elseif (\array_key_exists('outcome', $data) && $data['outcome'] === null) {
            $object->setOutcome(null);
        }
        if (\array_key_exists('principalId', $data) && $data['principalId'] !== null) {
            $object->setPrincipalId($data['principalId']);
        }
        elseif (\array_key_exists('principalId', $data) && $data['principalId'] === null) {
            $object->setPrincipalId(null);
        }
        if (\array_key_exists('userAgent', $data) && $data['userAgent'] !== null) {
            $object->setUserAgent($data['userAgent']);
        }
        elseif (\array_key_exists('userAgent', $data) && $data['userAgent'] === null) {
            $object->setUserAgent(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $dataArray['attemptType'] = $data->getAttemptType();
        $dataArray['attemptedAt'] = $data->getAttemptedAt()->format('Y-m-d\TH:i:sP');
        $dataArray['failureReason'] = $data->getFailureReason();
        $dataArray['id'] = $data->getId();
        $dataArray['identifier'] = $data->getIdentifier();
        $dataArray['ipAddress'] = $data->getIpAddress();
        $dataArray['outcome'] = $data->getOutcome();
        $dataArray['principalId'] = $data->getPrincipalId();
        $dataArray['userAgent'] = $data->getUserAgent();
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\LoginAttemptResponse::class => false];
    }
}