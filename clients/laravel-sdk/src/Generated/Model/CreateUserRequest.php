<?php

namespace FlowCatalyst\Generated\Model;

class CreateUserRequest extends \ArrayObject
{
    /**
     * @var array
     */
    protected $initialized = [];
    public function isInitialized($property): bool
    {
        return array_key_exists($property, $this->initialized);
    }
    /**
     * Client ID (for client-bound users)
     *
     * @var string|null
     */
    protected $clientId;
    /**
     * Email address
     *
     * @var string|null
     */
    protected $email;
    /**
     * Display name
     *
     * @var string|null
     */
    protected $name;
    /**
     * Password (optional - only for internal auth users)
     *
     * @var string|null
     */
    protected $password;
    /**
     * When false, the platform skips its password complexity rules (uppercase/lowercase/digit/special) and only enforces a 2-character minimum. Intended for SDK callers that apply their own policy. Defaults to true.
     *
     * @var bool|null
     */
    protected $enforcePasswordComplexity;
    /**
     * Client ID (for client-bound users)
     *
     * @return string|null
     */
    public function getClientId(): ?string
    {
        return $this->clientId;
    }
    /**
     * Client ID (for client-bound users)
     *
     * @param string|null $clientId
     *
     * @return self
     */
    public function setClientId(?string $clientId): self
    {
        $this->initialized['clientId'] = true;
        $this->clientId = $clientId;
        return $this;
    }
    /**
     * Email address
     *
     * @return string|null
     */
    public function getEmail(): ?string
    {
        return $this->email;
    }
    /**
     * Email address
     *
     * @param string|null $email
     *
     * @return self
     */
    public function setEmail(?string $email): self
    {
        $this->initialized['email'] = true;
        $this->email = $email;
        return $this;
    }
    /**
     * Display name
     *
     * @return string|null
     */
    public function getName(): ?string
    {
        return $this->name;
    }
    /**
     * Display name
     *
     * @param string|null $name
     *
     * @return self
     */
    public function setName(?string $name): self
    {
        $this->initialized['name'] = true;
        $this->name = $name;
        return $this;
    }
    /**
     * Password (optional - only for internal auth users)
     *
     * @return string|null
     */
    public function getPassword(): ?string
    {
        return $this->password;
    }
    /**
     * Password (optional - only for internal auth users)
     *
     * @param string|null $password
     *
     * @return self
     */
    public function setPassword(?string $password): self
    {
        $this->initialized['password'] = true;
        $this->password = $password;
        return $this;
    }
    /**
     * When false, the platform skips its password complexity rules (uppercase/lowercase/digit/special) and only enforces a 2-character minimum. Intended for SDK callers that apply their own policy. Defaults to true.
     *
     * @return bool|null
     */
    public function getEnforcePasswordComplexity(): ?bool
    {
        return $this->enforcePasswordComplexity;
    }
    /**
     * When false, the platform skips its password complexity rules (uppercase/lowercase/digit/special) and only enforces a 2-character minimum. Intended for SDK callers that apply their own policy. Defaults to true.
     *
     * @param bool|null $enforcePasswordComplexity
     *
     * @return self
     */
    public function setEnforcePasswordComplexity(?bool $enforcePasswordComplexity): self
    {
        $this->initialized['enforcePasswordComplexity'] = true;
        $this->enforcePasswordComplexity = $enforcePasswordComplexity;
        return $this;
    }
}