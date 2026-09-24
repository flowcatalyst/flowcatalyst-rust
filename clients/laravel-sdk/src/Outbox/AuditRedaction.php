<?php

declare(strict_types=1);

namespace FlowCatalyst\Outbox;

/**
 * Audit logs must never store passwords or secrets. `operationData` reaches
 * the outbox as the whole command; this walks it and masks anything that
 * looks like a credential before it is serialised into the audit row, so a
 * leak (a command that carries a plaintext token or secret field the way
 * `serviceaccount` Create/Update once did) never reaches `aud_logs`.
 *
 * A key is secret when, lower-cased with `_` and `-` removed, it **ends
 * with** `password`, `passwordhash`, `secret`, `secretref`, `passphrase` or
 * `token`, or **equals** `apikey`, `privatekey`, `authorization` or
 * `cookie`. A secret key's value becomes the string `"***"` — whatever its
 * type — except `null` and booleans, which are kept as-is. `$maskedFields`
 * additionally masks top-level field names the caller declares even though
 * the name rule would keep them (e.g. a config value whose secrecy depends
 * on a sibling field). Objects (associative arrays) and arrays are walked;
 * everything else is untouched. Pure — never mutates `$data`.
 */
final class AuditRedaction
{
    /** @var list<string> */
    private const SECRET_SUFFIXES = [
        'password',
        'passwordhash',
        'secret',
        'secretref',
        'passphrase',
        'token',
    ];

    /** @var list<string> */
    private const SECRET_EXACT = ['apikey', 'privatekey', 'authorization', 'cookie'];

    /**
     * @param array<string, mixed> $data
     * @param list<string> $maskedFields Top-level field names to mask in
     *   addition to the name rule.
     * @return array<string, mixed>
     */
    public static function redact(array $data, array $maskedFields = []): array
    {
        return self::redactObject($data, array_flip($maskedFields));
    }

    /**
     * @param array<string, mixed> $obj
     * @param array<string, int> $topLevelMasked
     * @return array<string, mixed>
     */
    private static function redactObject(array $obj, array $topLevelMasked): array
    {
        $result = [];
        foreach ($obj as $key => $value) {
            if (array_key_exists($key, $topLevelMasked) || self::isSecretKey((string) $key)) {
                $result[$key] = self::maskValue($value);
            } else {
                $result[$key] = self::redactValue($value);
            }
        }

        return $result;
    }

    private static function redactValue(mixed $value): mixed
    {
        // A nested object (a value object, a JsonSerializable) is walked as
        // the document json_encode will write for it, so a secret inside it
        // is caught too.
        if (is_object($value)) {
            $encoded = json_encode($value);
            if ($encoded !== false) {
                $value = json_decode($encoded, true);
            }
        }

        if (is_array($value)) {
            if (array_is_list($value)) {
                return array_map(fn (mixed $item): mixed => self::redactValue($item), $value);
            }

            return self::redactObject($value, []);
        }

        return $value;
    }

    private static function maskValue(mixed $value): mixed
    {
        if ($value === null || is_bool($value)) {
            return $value;
        }

        return '***';
    }

    private static function isSecretKey(string $key): bool
    {
        $normalized = str_replace(['_', '-'], '', strtolower($key));

        if (in_array($normalized, self::SECRET_EXACT, true)) {
            return true;
        }

        foreach (self::SECRET_SUFFIXES as $suffix) {
            if (str_ends_with($normalized, $suffix)) {
                return true;
            }
        }

        return false;
    }
}
