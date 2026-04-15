<?php

namespace FlowCatalyst\Generated\Model;

class ResetPasswordRequest extends \ArrayObject
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
     * New password (min 8 characters)
     *
     * @var string|null
     */
    protected $newPassword;
    /**
     * When false, the platform skips its password complexity rules (uppercase/lowercase/digit/special) and only enforces a 2-character minimum. Intended for SDK callers that apply their own policy. Defaults to true.
     *
     * @var bool|null
     */
    protected $enforcePasswordComplexity;
    /**
     * New password (min 8 characters)
     *
     * @return string|null
     */
    public function getNewPassword(): ?string
    {
        return $this->newPassword;
    }
    /**
     * New password (min 8 characters)
     *
     * @param string|null $newPassword
     *
     * @return self
     */
    public function setNewPassword(?string $newPassword): self
    {
        $this->initialized['newPassword'] = true;
        $this->newPassword = $newPassword;
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