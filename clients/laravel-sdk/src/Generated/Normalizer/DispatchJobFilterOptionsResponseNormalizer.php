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
class DispatchJobFilterOptionsResponseNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return $type === \FlowCatalyst\Generated\Model\DispatchJobFilterOptionsResponse::class;
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && get_class($data) === \FlowCatalyst\Generated\Model\DispatchJobFilterOptionsResponse::class;
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $object = new \FlowCatalyst\Generated\Model\DispatchJobFilterOptionsResponse();
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
        if (\array_key_exists('clientIds', $data) && $data['clientIds'] !== null) {
            $values = [];
            foreach ($data['clientIds'] as $value) {
                $values[] = $value;
            }
            $object->setClientIds($values);
        }
        elseif (\array_key_exists('clientIds', $data) && $data['clientIds'] === null) {
            $object->setClientIds(null);
        }
        if (\array_key_exists('codes', $data) && $data['codes'] !== null) {
            $values_1 = [];
            foreach ($data['codes'] as $value_1) {
                $values_1[] = $value_1;
            }
            $object->setCodes($values_1);
        }
        elseif (\array_key_exists('codes', $data) && $data['codes'] === null) {
            $object->setCodes(null);
        }
        if (\array_key_exists('dispatchPoolIds', $data) && $data['dispatchPoolIds'] !== null) {
            $values_2 = [];
            foreach ($data['dispatchPoolIds'] as $value_2) {
                $values_2[] = $value_2;
            }
            $object->setDispatchPoolIds($values_2);
        }
        elseif (\array_key_exists('dispatchPoolIds', $data) && $data['dispatchPoolIds'] === null) {
            $object->setDispatchPoolIds(null);
        }
        if (\array_key_exists('kinds', $data) && $data['kinds'] !== null) {
            $values_3 = [];
            foreach ($data['kinds'] as $value_3) {
                $values_3[] = $value_3;
            }
            $object->setKinds($values_3);
        }
        elseif (\array_key_exists('kinds', $data) && $data['kinds'] === null) {
            $object->setKinds(null);
        }
        if (\array_key_exists('statuses', $data) && $data['statuses'] !== null) {
            $values_4 = [];
            foreach ($data['statuses'] as $value_4) {
                $values_4[] = $value_4;
            }
            $object->setStatuses($values_4);
        }
        elseif (\array_key_exists('statuses', $data) && $data['statuses'] === null) {
            $object->setStatuses(null);
        }
        if (\array_key_exists('subscriptionIds', $data) && $data['subscriptionIds'] !== null) {
            $values_5 = [];
            foreach ($data['subscriptionIds'] as $value_5) {
                $values_5[] = $value_5;
            }
            $object->setSubscriptionIds($values_5);
        }
        elseif (\array_key_exists('subscriptionIds', $data) && $data['subscriptionIds'] === null) {
            $object->setSubscriptionIds(null);
        }
        return $object;
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $dataArray = [];
        $values = [];
        foreach ($data->getClientIds() as $value) {
            $values[] = $value;
        }
        $dataArray['clientIds'] = $values;
        $values_1 = [];
        foreach ($data->getCodes() as $value_1) {
            $values_1[] = $value_1;
        }
        $dataArray['codes'] = $values_1;
        $values_2 = [];
        foreach ($data->getDispatchPoolIds() as $value_2) {
            $values_2[] = $value_2;
        }
        $dataArray['dispatchPoolIds'] = $values_2;
        $values_3 = [];
        foreach ($data->getKinds() as $value_3) {
            $values_3[] = $value_3;
        }
        $dataArray['kinds'] = $values_3;
        $values_4 = [];
        foreach ($data->getStatuses() as $value_4) {
            $values_4[] = $value_4;
        }
        $dataArray['statuses'] = $values_4;
        $values_5 = [];
        foreach ($data->getSubscriptionIds() as $value_5) {
            $values_5[] = $value_5;
        }
        $dataArray['subscriptionIds'] = $values_5;
        return $dataArray;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [\FlowCatalyst\Generated\Model\DispatchJobFilterOptionsResponse::class => false];
    }
}