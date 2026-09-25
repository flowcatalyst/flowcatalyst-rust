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
class UpdateSubscriptionRequestNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\UpdateSubscriptionRequest::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\UpdateSubscriptionRequest::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\UpdateSubscriptionRequest();
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
        if (\array_key_exists('$schema', $data) && $data['$schema'] !== null) {
            $object->setDollarSchema($data['$schema']);
            unset($data['$schema']);
        }
        elseif (\array_key_exists('$schema', $data) && $data['$schema'] === null) {
            $object->setDollarSchema(null);
        }
        if (\array_key_exists('connectionId', $data) && $data['connectionId'] !== null) {
            $object->setConnectionId($data['connectionId']);
            unset($data['connectionId']);
        }
        elseif (\array_key_exists('connectionId', $data) && $data['connectionId'] === null) {
            $object->setConnectionId(null);
        }
        if (\array_key_exists('customConfig', $data) && $data['customConfig'] !== null) {
            $values = [];
            foreach ($data['customConfig'] as $value) {
                $values[] = $this->denormalizer->denormalize($value, \FlowCatalyst\Generated\Model\ConfigEntryDTO::class, 'json', $context);
            }
            $object->setCustomConfig($values);
            unset($data['customConfig']);
        }
        elseif (\array_key_exists('customConfig', $data) && $data['customConfig'] === null) {
            $object->setCustomConfig(null);
        }
        if (\array_key_exists('dataOnly', $data) && $data['dataOnly'] !== null) {
            $object->setDataOnly($data['dataOnly']);
            unset($data['dataOnly']);
        }
        elseif (\array_key_exists('dataOnly', $data) && $data['dataOnly'] === null) {
            $object->setDataOnly(null);
        }
        if (\array_key_exists('delaySeconds', $data) && $data['delaySeconds'] !== null) {
            $object->setDelaySeconds($data['delaySeconds']);
            unset($data['delaySeconds']);
        }
        elseif (\array_key_exists('delaySeconds', $data) && $data['delaySeconds'] === null) {
            $object->setDelaySeconds(null);
        }
        if (\array_key_exists('description', $data) && $data['description'] !== null) {
            $object->setDescription($data['description']);
            unset($data['description']);
        }
        elseif (\array_key_exists('description', $data) && $data['description'] === null) {
            $object->setDescription(null);
        }
        if (\array_key_exists('dispatchPoolId', $data) && $data['dispatchPoolId'] !== null) {
            $object->setDispatchPoolId($data['dispatchPoolId']);
            unset($data['dispatchPoolId']);
        }
        elseif (\array_key_exists('dispatchPoolId', $data) && $data['dispatchPoolId'] === null) {
            $object->setDispatchPoolId(null);
        }
        if (\array_key_exists('endpoint', $data) && $data['endpoint'] !== null) {
            $object->setEndpoint($data['endpoint']);
            unset($data['endpoint']);
        }
        elseif (\array_key_exists('endpoint', $data) && $data['endpoint'] === null) {
            $object->setEndpoint(null);
        }
        if (\array_key_exists('eventTypes', $data) && $data['eventTypes'] !== null) {
            $values_1 = [];
            foreach ($data['eventTypes'] as $value_1) {
                $values_1[] = $this->denormalizer->denormalize($value_1, \FlowCatalyst\Generated\Model\EventTypeBindingDTO::class, 'json', $context);
            }
            $object->setEventTypes($values_1);
            unset($data['eventTypes']);
        }
        elseif (\array_key_exists('eventTypes', $data) && $data['eventTypes'] === null) {
            $object->setEventTypes(null);
        }
        if (\array_key_exists('maxAgeSeconds', $data) && $data['maxAgeSeconds'] !== null) {
            $object->setMaxAgeSeconds($data['maxAgeSeconds']);
            unset($data['maxAgeSeconds']);
        }
        elseif (\array_key_exists('maxAgeSeconds', $data) && $data['maxAgeSeconds'] === null) {
            $object->setMaxAgeSeconds(null);
        }
        if (\array_key_exists('maxRetries', $data) && $data['maxRetries'] !== null) {
            $object->setMaxRetries($data['maxRetries']);
            unset($data['maxRetries']);
        }
        elseif (\array_key_exists('maxRetries', $data) && $data['maxRetries'] === null) {
            $object->setMaxRetries(null);
        }
        if (\array_key_exists('mode', $data) && $data['mode'] !== null) {
            $object->setMode($data['mode']);
            unset($data['mode']);
        }
        elseif (\array_key_exists('mode', $data) && $data['mode'] === null) {
            $object->setMode(null);
        }
        if (\array_key_exists('name', $data) && $data['name'] !== null) {
            $object->setName($data['name']);
            unset($data['name']);
        }
        elseif (\array_key_exists('name', $data) && $data['name'] === null) {
            $object->setName(null);
        }
        if (\array_key_exists('queue', $data) && $data['queue'] !== null) {
            $object->setQueue($data['queue']);
            unset($data['queue']);
        }
        elseif (\array_key_exists('queue', $data) && $data['queue'] === null) {
            $object->setQueue(null);
        }
        if (\array_key_exists('serviceAccountId', $data) && $data['serviceAccountId'] !== null) {
            $object->setServiceAccountId($data['serviceAccountId']);
            unset($data['serviceAccountId']);
        }
        elseif (\array_key_exists('serviceAccountId', $data) && $data['serviceAccountId'] === null) {
            $object->setServiceAccountId(null);
        }
        if (\array_key_exists('timeoutSeconds', $data) && $data['timeoutSeconds'] !== null) {
            $object->setTimeoutSeconds($data['timeoutSeconds']);
            unset($data['timeoutSeconds']);
        }
        elseif (\array_key_exists('timeoutSeconds', $data) && $data['timeoutSeconds'] === null) {
            $object->setTimeoutSeconds(null);
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
        if ($data->isInitialized('connectionId') && null !== $data->getConnectionId()) {
            $dataArray['connectionId'] = $data->getConnectionId();
        }
        if ($data->isInitialized('customConfig') && null !== $data->getCustomConfig()) {
            $values = [];
            foreach ($data->getCustomConfig() as $value) {
                $values[] = $this->normalizer->normalize($value, 'json', $context);
            }
            $dataArray['customConfig'] = $values;
        }
        if ($data->isInitialized('dataOnly') && null !== $data->getDataOnly()) {
            $dataArray['dataOnly'] = $data->getDataOnly();
        }
        if ($data->isInitialized('delaySeconds') && null !== $data->getDelaySeconds()) {
            $dataArray['delaySeconds'] = $data->getDelaySeconds();
        }
        if ($data->isInitialized('description') && null !== $data->getDescription()) {
            $dataArray['description'] = $data->getDescription();
        }
        if ($data->isInitialized('dispatchPoolId') && null !== $data->getDispatchPoolId()) {
            $dataArray['dispatchPoolId'] = $data->getDispatchPoolId();
        }
        if ($data->isInitialized('endpoint') && null !== $data->getEndpoint()) {
            $dataArray['endpoint'] = $data->getEndpoint();
        }
        if ($data->isInitialized('eventTypes') && null !== $data->getEventTypes()) {
            $values_1 = [];
            foreach ($data->getEventTypes() as $value_1) {
                $values_1[] = $this->normalizer->normalize($value_1, 'json', $context);
            }
            $dataArray['eventTypes'] = $values_1;
        }
        if ($data->isInitialized('maxAgeSeconds') && null !== $data->getMaxAgeSeconds()) {
            $dataArray['maxAgeSeconds'] = $data->getMaxAgeSeconds();
        }
        if ($data->isInitialized('maxRetries') && null !== $data->getMaxRetries()) {
            $dataArray['maxRetries'] = $data->getMaxRetries();
        }
        if ($data->isInitialized('mode') && null !== $data->getMode()) {
            $dataArray['mode'] = $data->getMode();
        }
        if ($data->isInitialized('name') && null !== $data->getName()) {
            $dataArray['name'] = $data->getName();
        }
        if ($data->isInitialized('queue') && null !== $data->getQueue()) {
            $dataArray['queue'] = $data->getQueue();
        }
        if ($data->isInitialized('serviceAccountId') && null !== $data->getServiceAccountId()) {
            $dataArray['serviceAccountId'] = $data->getServiceAccountId();
        }
        if ($data->isInitialized('timeoutSeconds') && null !== $data->getTimeoutSeconds()) {
            $dataArray['timeoutSeconds'] = $data->getTimeoutSeconds();
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
        return [\FlowCatalyst\Generated\Model\UpdateSubscriptionRequest::class => false];
    }
}