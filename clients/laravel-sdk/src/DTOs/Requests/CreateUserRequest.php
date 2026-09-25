<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for POST /api/principals/users.
 *
 * `password` is optional: omit it for OIDC-authenticated users.
 * `enforcePasswordComplexity` is accepted by the platform but not currently
 * wired into user creation — a supplied password always runs the full
 * password policy regardless of this flag (unlike resetPassword(), where
 * `false` does relax the check). Kept for wire compatibility; do not rely
 * on it to bypass complexity rules here.
 *
 * `scope` is optional and defaults to CLIENT. The platform never upgrades
 * it from the email domain: pass 'ANCHOR' or 'PARTNER' explicitly, and the
 * domain setup must back it (registered anchor domain / PARTNER
 * email-domain mapping) or the create is rejected.
 *
 * `clientId` accepts either the FlowCatalyst client id (`clt_…`) or the
 * client's identifier slug — the platform resolves both, so you can pass
 * your tenant code directly instead of listing clients to find the id.
 *
 * `sendInvitation` (default true) controls whether the platform emails the
 * new user at all — a passwordless user gets the "set your password"
 * invite, a user created with a password gets the "account created"
 * welcome. Set to false when your application is taking over the
 * invitation itself. Ignored for service-account creates.
 *
 * `returnInviteLink` (default false), when true, mints the 72-hour
 * "set your password" link and returns it as `Principal::$inviteLink`
 * instead of emailing it — only for a passwordless INTERNAL user (a no-op
 * otherwise). `returnInviteLink:true` always wins over `sendInvitation`:
 * the platform never sends its own invite email in that case, since the
 * link can only be minted once. The returned link is a live bearer
 * credential — treat it exactly like a password and never log it.
 *
 * `inviteRedirectUri` sends the invitee to your application once they have
 * set their password (and enrolled 2FA, if their domain requires it) — any
 * absolute http(s) URL, typically your app's own page, which then starts
 * sign-in as usual. It applies to both the platform-sent invite email and
 * `returnInviteLink`. A malformed value is rejected with
 * INVITE_REDIRECT_URI_INVALID and no user is created.
 */
final class CreateUserRequest
{
    public function __construct(
        public readonly string $email,
        public readonly string $name,
        public readonly ?string $password = null,
        public readonly ?string $clientId = null,
        public readonly ?bool $enforcePasswordComplexity = null,
        public readonly ?string $scope = null,
        public readonly ?bool $sendInvitation = null,
        public readonly ?bool $returnInviteLink = null,
        public readonly ?string $inviteRedirectUri = null,
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $payload = [
            'email' => $this->email,
            'name' => $this->name,
        ];
        if ($this->password !== null) {
            $payload['password'] = $this->password;
        }
        if ($this->clientId !== null) {
            $payload['clientId'] = $this->clientId;
        }
        if ($this->enforcePasswordComplexity !== null) {
            $payload['enforcePasswordComplexity'] = $this->enforcePasswordComplexity;
        }
        if ($this->scope !== null) {
            $payload['scope'] = $this->scope;
        }
        if ($this->sendInvitation !== null) {
            $payload['sendInvitation'] = $this->sendInvitation;
        }
        if ($this->returnInviteLink !== null) {
            $payload['returnInviteLink'] = $this->returnInviteLink;
        }
        if ($this->inviteRedirectUri !== null) {
            $payload['inviteRedirectUri'] = $this->inviteRedirectUri;
        }
        return $payload;
    }
}
