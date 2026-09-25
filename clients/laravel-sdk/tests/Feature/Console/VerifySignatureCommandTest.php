<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Feature\Console;

use FlowCatalyst\FlowCatalystServiceProvider;
use Illuminate\Console\Command;
use Orchestra\Testbench\TestCase;

/**
 * Pins `flowcatalyst:verify-signature`'s outcomes: the receiving-side check
 * an app runs against a delivery pasted from the platform's dispatch-job
 * "Sign" panel (see VerifySignatureCommand's docblock). Signing exactly as
 * WebhookValidatorCompatTest pins the Go side: hash_hmac('sha256', timestamp
 * . body, secret), millisecond-ISO8601 UTC timestamp.
 */
final class VerifySignatureCommandTest extends TestCase
{
    private const SECRET = 'test-secret-123';

    /** @var list<string> temp body files created by a test, cleaned up in tearDown */
    private array $tempFiles = [];

    protected function getPackageProviders($app): array
    {
        return [FlowCatalystServiceProvider::class];
    }

    protected function getEnvironmentSetUp($app): void
    {
        $app['config']->set('flowcatalyst.signing_secret', self::SECRET);
        $app['config']->set('flowcatalyst.base_url', 'https://fc.test');
    }

    protected function tearDown(): void
    {
        foreach ($this->tempFiles as $path) {
            if (is_string($path) && file_exists($path)) {
                unlink($path);
            }
        }
        $this->tempFiles = [];

        parent::tearDown();
    }

    /** Write $body to a fresh temp file and track it for cleanup. */
    private function bodyFile(string $body): string
    {
        $path = tempnam(sys_get_temp_dir(), 'fc-verify-sig-');
        file_put_contents($path, $body);
        $this->tempFiles[] = $path;
        return $path;
    }

    /** Millisecond-ISO8601 UTC "now", the same shape the platform emits. */
    private function nowTimestamp(): string
    {
        return gmdate('Y-m-d\TH:i:s') . '.000Z';
    }

    /** Sign exactly as the middleware/platform does: HMAC-SHA256(timestamp . body). */
    private function sign(string $timestamp, string $body, string $secret = self::SECRET): string
    {
        return hash_hmac('sha256', $timestamp . $body, $secret);
    }

    public function test_matching_signature_succeeds(): void
    {
        $body = '{"hello":"world"}';
        $timestamp = $this->nowTimestamp();
        $signature = $this->sign($timestamp, $body);

        $this->artisan('flowcatalyst:verify-signature', [
            '--timestamp' => $timestamp,
            '--signature' => $signature,
            '--body-file' => $this->bodyFile($body),
        ])
            ->assertExitCode(0)
            ->expectsOutputToContain('MATCH');
    }

    public function test_signature_from_a_different_secret_mismatches(): void
    {
        $body = '{"hello":"world"}';
        $timestamp = $this->nowTimestamp();
        $signature = $this->sign($timestamp, $body, 'a-different-secret');

        $this->artisan('flowcatalyst:verify-signature', [
            '--timestamp' => $timestamp,
            '--signature' => $signature,
            '--body-file' => $this->bodyFile($body),
        ])
            ->assertExitCode(1)
            ->expectsOutputToContain('MISMATCH');
    }

    public function test_matching_signature_with_stale_timestamp_warns_but_succeeds(): void
    {
        $body = '{"hello":"world"}';
        $timestamp = gmdate('Y-m-d\TH:i:s', time() - 600) . '.000Z'; // 10 minutes old, > default 300s tolerance
        $signature = $this->sign($timestamp, $body);

        $this->artisan('flowcatalyst:verify-signature', [
            '--timestamp' => $timestamp,
            '--signature' => $signature,
            '--body-file' => $this->bodyFile($body),
        ])
            ->assertExitCode(0)
            ->expectsOutputToContain('Signature matches, but');
    }

    public function test_missing_signature_option_is_invalid(): void
    {
        $this->artisan('flowcatalyst:verify-signature', [
            '--timestamp' => $this->nowTimestamp(),
            '--body-file' => $this->bodyFile('{"hello":"world"}'),
        ])
            ->assertExitCode(Command::INVALID);
    }

    public function test_unreadable_body_file_is_invalid(): void
    {
        $this->artisan('flowcatalyst:verify-signature', [
            '--timestamp' => $this->nowTimestamp(),
            '--signature' => 'deadbeef',
            '--body-file' => '/nonexistent/path/does-not-exist.json',
        ])
            ->assertExitCode(Command::INVALID);
    }

    public function test_empty_signing_secret_fails(): void
    {
        config(['flowcatalyst.signing_secret' => '']);

        $body = '{"hello":"world"}';
        $timestamp = $this->nowTimestamp();

        $this->artisan('flowcatalyst:verify-signature', [
            '--timestamp' => $timestamp,
            '--signature' => 'irrelevant',
            '--body-file' => $this->bodyFile($body),
        ])
            ->assertExitCode(1)
            ->expectsOutputToContain('FLOWCATALYST_SIGNING_SECRET is not set');
    }
}
