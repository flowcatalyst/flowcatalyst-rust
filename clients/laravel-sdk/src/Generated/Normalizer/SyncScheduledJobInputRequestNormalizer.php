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
class SyncScheduledJobInputRequestNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\SyncScheduledJobInputRequest::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\SyncScheduledJobInputRequest::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        if (isset($data['$ref'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        $object = new \FlowCatalyst\Generated\Model\SyncScheduledJobInputRequest();
        if (\array_key_exists('concurrent', $data) && \is_int($data['concurrent'])) {
            $data['concurrent'] = (bool) $data['concurrent'];
        }
        if (\array_key_exists('tracksCompletion', $data) && \is_int($data['tracksCompletion'])) {
            $data['tracksCompletion'] = (bool) $data['tracksCompletion'];
        }
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (\array_key_exists('code', $data) && $data['code'] !== null) {
            $object->setCode($data['code']);
            unset($data['code']);
        }
        elseif (\array_key_exists('code', $data) && $data['code'] === null) {
            $object->setCode(null);
        }
        if (\array_key_exists('concurrent', $data) && $data['concurrent'] !== null) {
            $object->setConcurrent($data['concurrent']);
            unset($data['concurrent']);
        }
        elseif (\array_key_exists('concurrent', $data) && $data['concurrent'] === null) {
            $object->setConcurrent(null);
        }
        if (\array_key_exists('crons', $data) && $data['crons'] !== null) {
            $values = [];
            foreach ($data['crons'] as $value) {
                $values[] = $value;
            }
            $object->setCrons($values);
            unset($data['crons']);
        }
        elseif (\array_key_exists('crons', $data) && $data['crons'] === null) {
            $object->setCrons(null);
        }
        if (\array_key_exists('deliveryMaxAttempts', $data) && $data['deliveryMaxAttempts'] !== null) {
            $object->setDeliveryMaxAttempts($data['deliveryMaxAttempts']);
            unset($data['deliveryMaxAttempts']);
        }
        elseif (\array_key_exists('deliveryMaxAttempts', $data) && $data['deliveryMaxAttempts'] === null) {
            $object->setDeliveryMaxAttempts(null);
        }
        if (\array_key_exists('description', $data) && $data['description'] !== null) {
            $object->setDescription($data['description']);
            unset($data['description']);
        }
        elseif (\array_key_exists('description', $data) && $data['description'] === null) {
            $object->setDescription(null);
        }
        if (\array_key_exists('name', $data) && $data['name'] !== null) {
            $object->setName($data['name']);
            unset($data['name']);
        }
        elseif (\array_key_exists('name', $data) && $data['name'] === null) {
            $object->setName(null);
        }
        if (\array_key_exists('payload', $data) && $data['payload'] !== null) {
            $object->setPayload($data['payload']);
            unset($data['payload']);
        }
        elseif (\array_key_exists('payload', $data) && $data['payload'] === null) {
            $object->setPayload(null);
        }
        if (\array_key_exists('targetUrl', $data) && $data['targetUrl'] !== null) {
            $object->setTargetUrl($data['targetUrl']);
            unset($data['targetUrl']);
        }
        elseif (\array_key_exists('targetUrl', $data) && $data['targetUrl'] === null) {
            $object->setTargetUrl(null);
        }
        if (\array_key_exists('timeoutSeconds', $data) && $data['timeoutSeconds'] !== null) {
            $object->setTimeoutSeconds($data['timeoutSeconds']);
            unset($data['timeoutSeconds']);
        }
        elseif (\array_key_exists('timeoutSeconds', $data) && $data['timeoutSeconds'] === null) {
            $object->setTimeoutSeconds(null);
        }
        if (\array_key_exists('timezone', $data) && $data['timezone'] !== null) {
            $object->setTimezone($data['timezone']);
            unset($data['timezone']);
        }
        elseif (\array_key_exists('timezone', $data) && $data['timezone'] === null) {
            $object->setTimezone(null);
        }
        if (\array_key_exists('tracksCompletion', $data) && $data['tracksCompletion'] !== null) {
            $object->setTracksCompletion($data['tracksCompletion']);
            unset($data['tracksCompletion']);
        }
        elseif (\array_key_exists('tracksCompletion', $data) && $data['tracksCompletion'] === null) {
            $object->setTracksCompletion(null);
        }
        foreach ($data as $key => $value_1) {
            if (preg_match('/.*/', (string) $key)) {
                $object[$key] = $value_1;
            }
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $dataArray['code'] = $data->getCode();
        if ($data->isInitialized('concurrent') && null !== $data->getConcurrent()) {
            $dataArray['concurrent'] = $data->getConcurrent();
        }
        $values = [];
        foreach ($data->getCrons() as $value) {
            $values[] = $value;
        }
        $dataArray['crons'] = $values;
        if ($data->isInitialized('deliveryMaxAttempts') && null !== $data->getDeliveryMaxAttempts()) {
            $dataArray['deliveryMaxAttempts'] = $data->getDeliveryMaxAttempts();
        }
        if ($data->isInitialized('description')) {
            $dataArray['description'] = $data->getDescription();
        }
        $dataArray['name'] = $data->getName();
        if ($data->isInitialized('payload') && null !== $data->getPayload()) {
            $dataArray['payload'] = $data->getPayload();
        }
        if ($data->isInitialized('targetUrl')) {
            $dataArray['targetUrl'] = $data->getTargetUrl();
        }
        if ($data->isInitialized('timeoutSeconds')) {
            $dataArray['timeoutSeconds'] = $data->getTimeoutSeconds();
        }
        if ($data->isInitialized('timezone') && null !== $data->getTimezone()) {
            $dataArray['timezone'] = $data->getTimezone();
        }
        if ($data->isInitialized('tracksCompletion') && null !== $data->getTracksCompletion()) {
            $dataArray['tracksCompletion'] = $data->getTracksCompletion();
        }
        foreach ($data as $key => $value_1) {
            if (preg_match('/.*/', (string) $key)) {
                $dataArray[$key] = $value_1;
            }
        }
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\SyncScheduledJobInputRequest::class => false];
    }
}