<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

use FlowCatalyst\Generated\Model\PrincipalDto;

class Principal
{
    /**
     * @param string[] $roles
     * @param string[] $grantedClientIds
     */
    public function __construct(
        public readonly string $id,
        public readonly string $type,
        public readonly ?string $scope,
        public readonly ?string $clientId,
        public readonly string $name,
        public readonly bool $active,
        public readonly ?string $email,
        public readonly ?string $idpType,
        public readonly array $roles,
        public readonly bool $isAnchorUser,
        public readonly array $grantedClientIds,
        public readonly ?string $createdAt,
        public readonly ?string $updatedAt,
        // Populated ONLY on a createUser response, and only when the
        // request asked for returnInviteLink:true (passwordless INTERNAL
        // user). Live 72h bearer credential — never log it.
        public readonly ?string $inviteLink = null,
    ) {}

    public static function fromGenerated(PrincipalDto $dto): self
    {
        return new self(
            id: $dto->getId(),
            type: $dto->getType(),
            scope: $dto->getScope(),
            clientId: $dto->getClientId(),
            name: $dto->getName(),
            active: $dto->getActive() ?? true,
            email: $dto->getEmail(),
            idpType: $dto->getIdpType(),
            roles: $dto->getRoles() ?? [],
            isAnchorUser: $dto->getIsAnchorUser() ?? false,
            grantedClientIds: $dto->getGrantedClientIds() ?? [],
            createdAt: $dto->getCreatedAt()?->format('c'),
            updatedAt: $dto->getUpdatedAt()?->format('c'),
            inviteLink: $dto->getInviteLink(),
        );
    }

    public static function fromArray(array $data): self
    {
        return new self(
            id: $data['id'],
            type: $data['type'],
            scope: $data['scope'] ?? null,
            clientId: $data['clientId'] ?? null,
            name: $data['name'],
            active: $data['active'] ?? true,
            email: $data['email'] ?? null,
            idpType: $data['idpType'] ?? null,
            roles: $data['roles'] ?? [],
            isAnchorUser: $data['isAnchorUser'] ?? false,
            grantedClientIds: $data['grantedClientIds'] ?? [],
            createdAt: $data['createdAt'] ?? null,
            updatedAt: $data['updatedAt'] ?? null,
            inviteLink: $data['inviteLink'] ?? null,
        );
    }

    public function toArray(): array
    {
        return [
            'id' => $this->id,
            'type' => $this->type,
            'scope' => $this->scope,
            'clientId' => $this->clientId,
            'name' => $this->name,
            'active' => $this->active,
            'email' => $this->email,
            'idpType' => $this->idpType,
            'roles' => $this->roles,
            'isAnchorUser' => $this->isAnchorUser,
            'grantedClientIds' => $this->grantedClientIds,
            'createdAt' => $this->createdAt,
            'updatedAt' => $this->updatedAt,
            'inviteLink' => $this->inviteLink,
        ];
    }

    /**
     * Check if this principal is a user.
     */
    public function isUser(): bool
    {
        return $this->type === 'USER';
    }

    /**
     * Check if this principal is a service account.
     */
    public function isServiceAccount(): bool
    {
        return $this->type === 'SERVICE';
    }

    /**
     * Check if this principal has anchor scope (platform admin).
     */
    public function isAnchor(): bool
    {
        return $this->scope === 'ANCHOR';
    }

    /**
     * Check if this principal has partner scope (multi-client access).
     */
    public function isPartner(): bool
    {
        return $this->scope === 'PARTNER';
    }

    /**
     * Check if this principal has client scope (single-client access).
     */
    public function isClient(): bool
    {
        return $this->scope === 'CLIENT';
    }

    /**
     * Check if this principal has a specific role.
     */
    public function hasRole(string $roleName): bool
    {
        return in_array($roleName, $this->roles, true);
    }
}
