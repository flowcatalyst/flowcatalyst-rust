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
class CreatedEventNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\CreatedEvent::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\CreatedEvent::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\CreatedEvent();
        if (null === $data || false === \is_array($data)) {
            return $object;
        }
        if (isset($data['$ref']) && !isset($data['type']) && !isset($data['properties']) && !isset($data['allOf'])) {
            return new Reference($data['$ref'], $context['document-origin']);
        }
        if (isset($data['$recursiveRef'])) {
            return new Reference($data['$recursiveRef'], $context['document-origin']);
        }
        if (\array_key_exists('causationId', $data) && $data['causationId'] !== null) {
            $object->setCausationId($data['causationId']);
        }
        elseif (\array_key_exists('causationId', $data) && $data['causationId'] === null) {
            $object->setCausationId(null);
        }
        if (\array_key_exists('clientId', $data) && $data['clientId'] !== null) {
            $object->setClientId($data['clientId']);
        }
        elseif (\array_key_exists('clientId', $data) && $data['clientId'] === null) {
            $object->setClientId(null);
        }
        if (\array_key_exists('contextData', $data) && $data['contextData'] !== null) {
            $values = [];
            foreach ($data['contextData'] as $value) {
                $values[] = $this->denormalizer->denormalize($value, \FlowCatalyst\Generated\Model\ContextEntryDTO::class, 'json', $context);
            }
            $object->setContextData($values);
        }
        elseif (\array_key_exists('contextData', $data) && $data['contextData'] === null) {
            $object->setContextData(null);
        }
        if (\array_key_exists('correlationId', $data) && $data['correlationId'] !== null) {
            $object->setCorrelationId($data['correlationId']);
        }
        elseif (\array_key_exists('correlationId', $data) && $data['correlationId'] === null) {
            $object->setCorrelationId(null);
        }
        if (\array_key_exists('createdAt', $data) && $data['createdAt'] !== null) {
            $object->setCreatedAt(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['createdAt']));
        }
        elseif (\array_key_exists('createdAt', $data) && $data['createdAt'] === null) {
            $object->setCreatedAt(null);
        }
        if (\array_key_exists('data', $data) && $data['data'] !== null) {
            $object->setData($data['data']);
        }
        elseif (\array_key_exists('data', $data) && $data['data'] === null) {
            $object->setData(null);
        }
        if (\array_key_exists('deduplicationId', $data) && $data['deduplicationId'] !== null) {
            $object->setDeduplicationId($data['deduplicationId']);
        }
        elseif (\array_key_exists('deduplicationId', $data) && $data['deduplicationId'] === null) {
            $object->setDeduplicationId(null);
        }
        if (\array_key_exists('eventType', $data) && $data['eventType'] !== null) {
            $object->setEventType($data['eventType']);
        }
        elseif (\array_key_exists('eventType', $data) && $data['eventType'] === null) {
            $object->setEventType(null);
        }
        if (\array_key_exists('id', $data) && $data['id'] !== null) {
            $object->setId($data['id']);
        }
        elseif (\array_key_exists('id', $data) && $data['id'] === null) {
            $object->setId(null);
        }
        if (\array_key_exists('messageGroup', $data) && $data['messageGroup'] !== null) {
            $object->setMessageGroup($data['messageGroup']);
        }
        elseif (\array_key_exists('messageGroup', $data) && $data['messageGroup'] === null) {
            $object->setMessageGroup(null);
        }
        if (\array_key_exists('source', $data) && $data['source'] !== null) {
            $object->setSource($data['source']);
        }
        elseif (\array_key_exists('source', $data) && $data['source'] === null) {
            $object->setSource(null);
        }
        if (\array_key_exists('specVersion', $data) && $data['specVersion'] !== null) {
            $object->setSpecVersion($data['specVersion']);
        }
        elseif (\array_key_exists('specVersion', $data) && $data['specVersion'] === null) {
            $object->setSpecVersion(null);
        }
        if (\array_key_exists('subject', $data) && $data['subject'] !== null) {
            $object->setSubject($data['subject']);
        }
        elseif (\array_key_exists('subject', $data) && $data['subject'] === null) {
            $object->setSubject(null);
        }
        if (\array_key_exists('time', $data) && $data['time'] !== null) {
            $object->setTime(\DateTime::createFromFormat('Y-m-d\TH:i:sP', $data['time']));
        }
        elseif (\array_key_exists('time', $data) && $data['time'] === null) {
            $object->setTime(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        if ($data->isInitialized('causationId') && null !== $data->getCausationId()) {
            $dataArray['causationId'] = $data->getCausationId();
        }
        if ($data->isInitialized('clientId') && null !== $data->getClientId()) {
            $dataArray['clientId'] = $data->getClientId();
        }
        if ($data->isInitialized('contextData') && null !== $data->getContextData()) {
            $values = [];
            foreach ($data->getContextData() as $value) {
                $values[] = $this->normalizer->normalize($value, 'json', $context);
            }
            $dataArray['contextData'] = $values;
        }
        if ($data->isInitialized('correlationId') && null !== $data->getCorrelationId()) {
            $dataArray['correlationId'] = $data->getCorrelationId();
        }
        $dataArray['createdAt'] = $data->getCreatedAt()->format('Y-m-d\TH:i:sP');
        $dataArray['data'] = $data->getData();
        if ($data->isInitialized('deduplicationId') && null !== $data->getDeduplicationId()) {
            $dataArray['deduplicationId'] = $data->getDeduplicationId();
        }
        $dataArray['eventType'] = $data->getEventType();
        $dataArray['id'] = $data->getId();
        if ($data->isInitialized('messageGroup') && null !== $data->getMessageGroup()) {
            $dataArray['messageGroup'] = $data->getMessageGroup();
        }
        $dataArray['source'] = $data->getSource();
        $dataArray['specVersion'] = $data->getSpecVersion();
        if ($data->isInitialized('subject') && null !== $data->getSubject()) {
            $dataArray['subject'] = $data->getSubject();
        }
        $dataArray['time'] = $data->getTime()->format('Y-m-d\TH:i:sP');
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\CreatedEvent::class => false];
    }
}