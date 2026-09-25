<?php

declare(strict_types=1);

namespace FlowCatalyst\Console\Commands;

use FlowCatalyst\Exceptions\WebhookValidationException;
use FlowCatalyst\Webhook\WebhookValidator;
use Illuminate\Console\Command;

/**
 * Verify a FlowCatalyst delivery signature against THIS app's configured
 * secret — the receiving half of the platform's "Sign" action on a dispatch
 * job.
 *
 * The platform's dispatch-job panel builds a delivery without sending it and
 * shows the timestamp, the signature and the exact body it signed. Paste
 * those three here and this command computes what `flowcatalyst.webhook`
 * would have computed for them with `FLOWCATALYST_SIGNING_SECRET`, and says
 * whether they match. A mismatch with a well-formed timestamp means the two
 * sides hold different secrets; nothing else about the delivery matters.
 *
 *   php artisan flowcatalyst:verify-signature \
 *       --timestamp='2026-09-22T15:16:54.010Z' \
 *       --signature='9f3a…' \
 *       --body-file=body.json
 *
 * The body must be byte-for-byte what was signed — save it from the panel
 * to a file rather than re-typing or re-formatting it; a single re-indented
 * space changes the HMAC.
 */
class VerifySignatureCommand extends Command
{
    protected $signature = 'flowcatalyst:verify-signature
        {--timestamp= : The X-FlowCatalyst-Timestamp value the signature covers}
        {--signature= : The X-FlowCatalyst-Signature value to check}
        {--body-file= : File holding the exact request body (or - for stdin)}
        {--tolerance=300 : Replay window in seconds, as the middleware applies it}';

    protected $description = 'Check a delivery timestamp + signature + body against FLOWCATALYST_SIGNING_SECRET';

    public function handle(): int
    {
        $timestamp = (string) $this->option('timestamp');
        $signature = strtolower(trim((string) $this->option('signature')));
        $bodyFile = (string) $this->option('body-file');

        if ($timestamp === '' || $signature === '' || $bodyFile === '') {
            $this->error('--timestamp, --signature and --body-file are all required.');
            return self::INVALID;
        }
        $body = $bodyFile === '-' ? file_get_contents('php://stdin') : @file_get_contents($bodyFile);
        if ($body === false) {
            $this->error("Could not read body from {$bodyFile}.");
            return self::INVALID;
        }

        $secret = (string) config('flowcatalyst.signing_secret');
        if ($secret === '') {
            $this->error('FLOWCATALYST_SIGNING_SECRET is not set — the middleware would answer 500 to every delivery.');
            return self::FAILURE;
        }

        // Exactly the middleware's computation: HMAC-SHA256 over timestamp . body.
        $expected = hash_hmac('sha256', $timestamp . $body, $secret);

        $this->line('Secret in use      : ' . self::fingerprint($secret));
        $this->line('Body               : ' . strlen($body) . ' bytes, sha256 ' . substr(hash('sha256', $body), 0, 16) . '…');
        $this->line('Timestamp          : ' . $timestamp);
        $this->line('Signature presented: ' . $signature);
        $this->line('Signature expected : ' . $expected);

        // The replay window, reported separately: a stale timestamp is a
        // different failure from a wrong secret, and the panel's signature is
        // minted at click time, so this only bites if the paste is slow.
        $tolerance = max(0, (int) $this->option('tolerance'));
        try {
            (new WebhookValidator($secret))->validate($body, $signature, $timestamp, $tolerance);
            $this->info('MATCH — this app\'s secret accepts the signature; the middleware would pass this delivery.');
            return self::SUCCESS;
        } catch (WebhookValidationException $e) {
            if (hash_equals($expected, $signature)) {
                // Signature fine, timestamp not: the secret is right.
                $this->warn('Signature matches, but: ' . $e->getMessage());
                $this->line('The secret is correct. Re-sign on the platform and verify promptly, or raise --tolerance for this check.');
                return self::SUCCESS;
            }
            $this->error('MISMATCH — ' . $e->getMessage());
            $this->line('This app\'s FLOWCATALYST_SIGNING_SECRET is not the secret the platform signed with.');
            $this->line('On the platform, the dispatch job\'s Sign panel names the service account that signed; copy THAT account\'s signing secret here, then `php artisan config:clear`.');
            return self::FAILURE;
        }
    }

    /**
     * A non-reversible hint of which secret is loaded, for comparing two
     * environments without printing either secret.
     */
    private static function fingerprint(string $secret): string
    {
        return 'sha256:' . substr(hash('sha256', $secret), 0, 12) . ' (' . strlen($secret) . ' chars)';
    }
}
