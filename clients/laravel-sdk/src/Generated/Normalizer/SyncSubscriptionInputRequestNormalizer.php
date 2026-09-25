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
class SyncSubscriptionInputRequestNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\SyncSubscriptionInputRequest::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\SyncSubscriptionInputRequest::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\SyncSubscriptionInputRequest();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('dataOnly', $data) && \is_int($data['dataOnly'])) {
            $data['dataOnly'] = (bool) $data['dataOnly'];
        }
        if (\array_key_exists('code', $data) && $data['code'] !== null) {
            $object->setCode($data['code']);
        }
        elseif (\array_key_exists('code', $data) && $data['code'] === null) {
            $object->setCode(null);
        }
        if (\array_key_exists('connectionId', $data) && $data['connectionId'] !== null) {
            $object->setConnectionId($data['connectionId']);
        }
        elseif (\array_key_exists('connectionId', $data) && $data['connectionId'] === null) {
            $object->setConnectionId(null);
        }
        if (\array_key_exists('dataOnly', $data) && $data['dataOnly'] !== null) {
            $object->setDataOnly($data['dataOnly']);
        }
        elseif (\array_key_exists('dataOnly', $data) && $data['dataOnly'] === null) {
            $object->setDataOnly(null);
        }
        if (\array_key_exists('description', $data) && $data['description'] !== null) {
            $object->setDescription($data['description']);
        }
        elseif (\array_key_exists('description', $data) && $data['description'] === null) {
            $object->setDescription(null);
        }
        if (\array_key_exists('dispatchPoolCode', $data) && $data['dispatchPoolCode'] !== null) {
            $object->setDispatchPoolCode($data['dispatchPoolCode']);
        }
        elseif (\array_key_exists('dispatchPoolCode', $data) && $data['dispatchPoolCode'] === null) {
            $object->setDispatchPoolCode(null);
        }
        if (\array_key_exists('eventTypes', $data) && $data['eventTypes'] !== null) {
            $values = [];
            foreach ($data['eventTypes'] as $value) {
                $values[] = $this->denormalizer->denormalize($value, \FlowCatalyst\Generated\Model\SyncSubscriptionEventTypeRequest::class, 'json', $context);
            }
            $object->setEventTypes($values);
        }
        elseif (\array_key_exists('eventTypes', $data) && $data['eventTypes'] === null) {
            $object->setEventTypes(null);
        }
        if (\array_key_exists('maxRetries', $data) && $data['maxRetries'] !== null) {
            $object->setMaxRetries($data['maxRetries']);
        }
        elseif (\array_key_exists('maxRetries', $data) && $data['maxRetries'] === null) {
            $object->setMaxRetries(null);
        }
        if (\array_key_exists('mode', $data) && $data['mode'] !== null) {
            $object->setMode($data['mode']);
        }
        elseif (\array_key_exists('mode', $data) && $data['mode'] === null) {
            $object->setMode(null);
        }
        if (\array_key_exists('name', $data) && $data['name'] !== null) {
            $object->setName($data['name']);
        }
        elseif (\array_key_exists('name', $data) && $data['name'] === null) {
            $object->setName(null);
        }
        if (\array_key_exists('target', $data) && $data['target'] !== null) {
            $object->setTarget($data['target']);
        }
        elseif (\array_key_exists('target', $data) && $data['target'] === null) {
            $object->setTarget(null);
        }
        if (\array_key_exists('timeoutSeconds', $data) && $data['timeoutSeconds'] !== null) {
            $object->setTimeoutSeconds($data['timeoutSeconds']);
        }
        elseif (\array_key_exists('timeoutSeconds', $data) && $data['timeoutSeconds'] === null) {
            $object->setTimeoutSeconds(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $dataArray['code'] = $data->getCode();
        if ($data->isInitialized('connectionId') && null !== $data->getConnectionId()) {
            $dataArray['connectionId'] = $data->getConnectionId();
        }
        if ($data->isInitialized('dataOnly') && null !== $data->getDataOnly()) {
            $dataArray['dataOnly'] = $data->getDataOnly();
        }
        if ($data->isInitialized('description') && null !== $data->getDescription()) {
            $dataArray['description'] = $data->getDescription();
        }
        if ($data->isInitialized('dispatchPoolCode') && null !== $data->getDispatchPoolCode()) {
            $dataArray['dispatchPoolCode'] = $data->getDispatchPoolCode();
        }
        $values = [];
        foreach ($data->getEventTypes() as $value) {
            $values[] = $this->normalizer->normalize($value, 'json', $context);
        }
        $dataArray['eventTypes'] = $values;
        if ($data->isInitialized('maxRetries') && null !== $data->getMaxRetries()) {
            $dataArray['maxRetries'] = $data->getMaxRetries();
        }
        if ($data->isInitialized('mode') && null !== $data->getMode()) {
            $dataArray['mode'] = $data->getMode();
        }
        $dataArray['name'] = $data->getName();
        $dataArray['target'] = $data->getTarget();
        if ($data->isInitialized('timeoutSeconds') && null !== $data->getTimeoutSeconds()) {
            $dataArray['timeoutSeconds'] = $data->getTimeoutSeconds();
        }
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\SyncSubscriptionInputRequest::class => false];
    }
}