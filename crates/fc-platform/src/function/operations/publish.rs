//! Java `function/operations/PublishVersion.java` (`:96-229`): a new,
//! immutable version of an existing function.
//!
//! In order, as Java: load and reach (`404`), `FUNCTION_DISABLED`, the
//! artifact ref and digest, a `platform://` ref's own checks, the owner's
//! policy and ceilings, the strict manifest, the signature, a duplicate
//! digest, the publish checks, then the next version number and the commit.
//!
//! **A duplicate digest** (`VERSION_DIGEST_EXISTS`, `details.version`) is a
//! no-op when the normalised manifest is the existing version's too (owner
//! decision 5, beyond Java): the use case stops before its commit with
//! [`UseCaseError::unchanged`], and the handler answers `200` with that
//! version. Under a different manifest it stays Java's `409`.
//! Java runs the publish checks after the version row is written and rolls
//! it back on a failure; they only read, so running them just before the
//! number is reserved gives the same outcome and the same first error,
//! without holding the function's row lock while they run.
//!
//! Like Java's `TxOperation`, this runs on a transaction-scoped unit of
//! work (`PgUnitOfWork::run`): the next version number is read under the
//! function's row lock ([`NextVersionOf`]) in the same transaction the
//! version is committed in, so concurrent publishes serialise to `n` and
//! `n + 1`. On any other unit of work the locked read refuses.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use fc_function_signing::{Signatures, Verification};
use serde::Serialize;

use super::access::{function_by_address, Caller};
use super::events::VersionPublished;
use super::publish_checks::PublishChecks;
use crate::function::artifact::{self, ArtifactBlobStore, PlatformArtifactRef};
use crate::function::entity::{
    ClientPolicy, Function, FunctionStatus, FunctionVersion, SignerIdentity,
};
use crate::function::policy_repository::ClientPolicyRepository;
use crate::function::repository::FunctionRepository;
use crate::function::version_repository::{FunctionVersionRepository, NextVersionOf};
use crate::function::{
    java_is_blank, ClientCeilings, Digest, FunctionAddress, FunctionLimits, JsonNode, Manifest,
    Runtime,
};
use crate::usecase::{
    AuditMasked, ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

/// `POST /api/functions/{address}/versions`. The audit row carries the
/// command as Java's does: the bundle and manifest included, masked only by
/// the platform's secret-name rule.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishCommand {
    #[serde(serialize_with = "super::serialize_address")]
    pub address: FunctionAddress,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_bundle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<JsonNode>,
}

impl AuditMasked for PublishCommand {}

/// `^(oci|file|s3|platform)://.+`, as the code has it (the spec lists
/// fewer schemes). Java's `.` matches no line terminator.
fn validate_artifact_ref(artifact_ref: &str) -> Result<(), UseCaseError> {
    let valid = ["oci://", "file://", "s3://", "platform://"]
        .iter()
        .find_map(|scheme| artifact_ref.strip_prefix(scheme))
        .is_some_and(|rest| {
            !rest.is_empty() && !rest.contains(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}'])
        });
    if valid {
        Ok(())
    } else {
        Err(UseCaseError::validation(
            "ARTIFACT_REF_INVALID",
            "artifactRef must start with oci://, file://, s3://, or platform://",
        ))
    }
}

pub struct PublishVersionUseCase<U: UnitOfWork> {
    pub(crate) functions: Arc<FunctionRepository>,
    pub(crate) versions: Arc<FunctionVersionRepository>,
    pub(crate) policies: Arc<ClientPolicyRepository>,
    pub(crate) limits: FunctionLimits,
    pub(crate) signatures: Signatures,
    pub(crate) artifacts: Option<Arc<dyn ArtifactBlobStore>>,
    pub(crate) checks: PublishChecks,
    pub(crate) unit_of_work: Arc<U>,
    pub(crate) caller: Caller,
}

/// Everything decided before a version number is taken.
struct Prepared {
    function: Function,
    digest: Digest,
    artifact_ref: String,
    manifest: Manifest,
    /// Stored as sent, verified or not.
    signature_bundle: Option<String>,
    signer: Option<SignerIdentity>,
}

#[async_trait]
impl<U: UnitOfWork> UseCase for PublishVersionUseCase<U> {
    type Command = PublishCommand;
    type Event = VersionPublished;

    /// The checks that need no read: the ref's presence and shape, the
    /// digest's shape.
    async fn validate(&self, command: &PublishCommand) -> Result<(), UseCaseError> {
        let artifact_ref = command.artifact_ref.as_deref().unwrap_or("");
        if java_is_blank(artifact_ref) {
            return Err(UseCaseError::validation(
                "ARTIFACT_REF_REQUIRED",
                "artifactRef is required",
            ));
        }
        validate_artifact_ref(artifact_ref)?;
        Digest::parse(command.digest.as_deref().unwrap_or(""))?;
        Ok(())
    }

    /// Load-or-404 and reach are in `execute`, after the load.
    async fn authorize(
        &self,
        _command: &PublishCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: PublishCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<VersionPublished> {
        let (version, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };
        self.unit_of_work
            .commit(&version, &*self.versions, event, &command)
            .await
    }
}

impl<U: UnitOfWork> PublishVersionUseCase<U> {
    /// Everything before the commit: Java's steps 1-7.
    async fn prepare(
        &self,
        command: &PublishCommand,
        ctx: &ExecutionContext,
    ) -> Result<(FunctionVersion, VersionPublished), UseCaseError> {
        let function = function_by_address(&self.functions, &command.address, &self.caller).await?;
        if function.status == FunctionStatus::Disabled {
            return Err(UseCaseError::business_rule(
                "FUNCTION_DISABLED",
                "function is disabled",
            ));
        }

        let artifact_ref = command.artifact_ref.clone().unwrap_or_default();
        validate_artifact_ref(&artifact_ref)?;
        let digest = Digest::parse(command.digest.as_deref().unwrap_or(""))?;
        check_platform_ref(
            self.artifacts.as_deref(),
            &artifact_ref,
            &function.id,
            &digest,
        )
        .await?;

        let policy = self.policies.find_by_owner(&function.owner).await?;
        // No policy row: the platform defaults.
        let ceilings = match &policy {
            Some(p) => p
                .ceilings(&self.limits)
                .map_err(|e| UseCaseError::internal("CEILINGS_INVALID", e.to_string()))?,
            None => ClientCeilings::of(&self.limits),
        };

        let manifest = Manifest::parse_strict(
            command.manifest.as_ref(),
            function.runtime,
            &self.limits,
            &ceilings,
        )?;

        let signer = resolve_signer(
            &self.signatures,
            command.signature_bundle.as_deref(),
            &digest,
            policy.as_ref(),
            function.runtime,
        )?;

        let p = Prepared {
            function,
            digest,
            artifact_ref,
            manifest,
            signature_bundle: command.signature_bundle.clone(),
            signer,
        };
        self.stage(&p, ctx).await
    }

    /// Java's step 6, the publish checks, and step 7's number, reserved
    /// under the function's row lock: the version and its event, ready to
    /// commit.
    async fn stage(
        &self,
        p: &Prepared,
        ctx: &ExecutionContext,
    ) -> Result<(FunctionVersion, VersionPublished), UseCaseError> {
        if let Some(existing) = self
            .versions
            .find_by_function_and_digest(&p.function.id, &p.digest)
            .await?
        {
            let mut details = HashMap::new();
            details.insert("version".to_string(), serde_json::json!(existing.version));
            let message = format!(
                "digest is already published as version {} for this function",
                existing.version
            );
            // The same bytes with the same (normalised) manifest are the
            // version that already exists: a no-op, which the handler
            // answers 200 with that version. The same bytes under another
            // manifest stay a conflict: a version is immutable.
            if existing.manifest == p.manifest {
                return Err(UseCaseError::unchanged(
                    "VERSION_DIGEST_EXISTS",
                    message,
                    details,
                ));
            }
            return Err(UseCaseError::business_rule_with_details(
                "VERSION_DIGEST_EXISTS",
                message,
                details,
            ));
        }
        let problems = self
            .checks
            .check(&p.function, &p.manifest, &self.caller)
            .await?;
        if let Some(first) = problems.into_iter().next() {
            return Err(first);
        }
        let number = self
            .unit_of_work
            .read_locked(&*self.versions, &NextVersionOf(p.function.id.clone()))
            .await?;
        let version = FunctionVersion::publish(
            &p.function.id,
            number,
            &p.artifact_ref,
            p.digest.clone(),
            p.signature_bundle.clone(),
            p.signer.clone(),
            p.manifest.clone(),
            &ctx.principal_id,
            Utc::now(),
        );
        let event = VersionPublished::new(ctx, &p.function, &version);
        Ok((version, event))
    }
}

/// A `platform://` ref must name this function and this digest
/// (`ARTIFACT_REF_MISMATCH`) and have been uploaded
/// (`ARTIFACT_NOT_UPLOADED`); without a store neither can be checked
/// (`503`). Any other scheme passes.
async fn check_platform_ref(
    store: Option<&dyn ArtifactBlobStore>,
    artifact_ref: &str,
    function_id: &str,
    digest: &Digest,
) -> Result<(), UseCaseError> {
    if !artifact_ref.starts_with("platform://") {
        return Ok(());
    }
    let store = store.ok_or_else(artifact::store_not_configured)?;
    let reference = PlatformArtifactRef::parse(artifact_ref)
        .filter(|r| r.function_id == function_id && r.hex == digest.hex())
        .ok_or_else(artifact::ref_mismatch)?;
    let exists = store
        .exists(&reference.function_id, digest)
        .await
        .map_err(|e| {
            tracing::error!(%function_id, error = %e, "checking an uploaded artifact failed");
            UseCaseError::internal(
                "ARTIFACT_STORE_ERROR",
                "checking the uploaded artifact failed",
            )
        })?;
    if !exists {
        return Err(artifact::not_uploaded());
    }
    Ok(())
}

/// Java's step 5. `Off` stores whatever bundle was sent, unverified, with
/// no signer. `Required` demands a bundle, verifies it against the digest
/// and the trust root, then requires the owner's policy to permit that
/// signer for this runtime; an absent policy permits nothing.
fn resolve_signer(
    signatures: &Signatures,
    bundle: Option<&str>,
    digest: &Digest,
    policy: Option<&ClientPolicy>,
    runtime: Runtime,
) -> Result<Option<SignerIdentity>, UseCaseError> {
    let Signatures::Required(verifier) = signatures else {
        return Ok(None);
    };
    let Some(bundle) = bundle.filter(|b| !java_is_blank(b)) else {
        return Err(UseCaseError::validation(
            "SIGNATURE_REQUIRED",
            "a signature bundle is required to publish",
        ));
    };
    let expected = fc_function_signing::Digest::parse(digest.value())
        .expect("a platform digest is a valid signing digest");
    match verifier.verify(Some(bundle), &expected) {
        Verification::Rejected { reason, detail } => {
            let mut details = HashMap::new();
            details.insert("reason".to_string(), serde_json::json!(reason.name()));
            Err(UseCaseError::validation_with_details(
                "SIGNATURE_REJECTED",
                format!("signature rejected: {detail}"),
                details,
            ))
        }
        Verification::Verified { signer, .. } => {
            let signer = SignerIdentity {
                issuer: signer.issuer,
                subject: signer.subject,
            };
            if !policy.is_some_and(|p| p.permits(&signer, runtime)) {
                return Err(UseCaseError::forbidden(
                    "SIGNER_NOT_PERMITTED",
                    format!(
                        "signer not permitted to publish: issuer='{}' subject='{}'",
                        signer.issuer, signer.subject
                    ),
                ));
            }
            Ok(Some(signer))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::function::artifact::FileArtifactBlobStore;
    use crate::function::entity::SignerRule;
    use crate::function::FunctionOwner;
    use fc_function_signing::{SignatureVerifier, TrustRoot};
    use sha2::{Digest as _, Sha256};

    /// Java's golden Sigstore bundle (`server/src/test/resources/function/
    /// sigstore/`), signed by the conformance beacon over `artifact.txt`.
    const BUNDLE: &str =
        include_str!("../../../tests/data/function/sigstore/happy-path-v0.3.sigstore.json");
    const ARTIFACT: &[u8] = include_bytes!("../../../tests/data/function/sigstore/artifact.txt");
    const ISSUER: &str = "https://token.actions.githubusercontent.com";
    const SUBJECT: &str =
        "https://github.com/sigstore-conformance/extremely-dangerous-public-oidc-beacon/\
                           .github/workflows/extremely-dangerous-oidc-beacon.yml@refs/heads/main";

    fn digest_of(bytes: &[u8]) -> Digest {
        Digest::from_sha256(&Sha256::digest(bytes).into())
    }

    fn required() -> Signatures {
        Signatures::Required(Arc::new(SignatureVerifier::new(
            TrustRoot::sigstore_public_good(),
        )))
    }

    fn policy(rules: Vec<SignerRule>) -> ClientPolicy {
        ClientPolicy {
            owner: FunctionOwner::Platform,
            signers: rules,
            max_duration_ms: None,
            max_concurrency: None,
            max_wasm_memory_mb: None,
            max_db_pool_size: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn code(r: Result<Option<SignerIdentity>, UseCaseError>) -> (u16, String) {
        let e = r.unwrap_err();
        (e.http_status_code(), e.code().to_string())
    }

    /// Java `PublishSignaturesTest`, clause by clause (P8).
    #[test]
    fn required_signatures_verify_then_ask_the_owners_policy() {
        let sig = required();
        let digest = digest_of(ARTIFACT);
        let exact = policy(vec![SignerRule::new(ISSUER, SUBJECT, [Runtime::Wasm])]);

        // No bundle, or a blank one.
        for bundle in [None, Some(" \n")] {
            let err =
                resolve_signer(&sig, bundle, &digest, Some(&exact), Runtime::Wasm).unwrap_err();
            assert_eq!(
                (err.http_status_code(), err.code(), err.message()),
                (
                    400,
                    "SIGNATURE_REQUIRED",
                    "a signature bundle is required to publish"
                )
            );
        }

        // A bundle over another digest: rejected, naming the reason.
        let other = digest_of(b"a completely different artifact");
        let err =
            resolve_signer(&sig, Some(BUNDLE), &other, Some(&exact), Runtime::Wasm).unwrap_err();
        assert_eq!(
            (err.http_status_code(), err.code()),
            (400, "SIGNATURE_REJECTED")
        );
        assert!(
            err.message().starts_with("signature rejected: "),
            "{}",
            err.message()
        );
        assert_eq!(err.details()["reason"], "DIGEST_MISMATCH");

        let err =
            resolve_signer(&sig, Some("{"), &digest, Some(&exact), Runtime::Wasm).unwrap_err();
        assert_eq!(err.details()["reason"], "MALFORMED_BUNDLE");

        // A valid bundle and no policy row: an absent policy permits nothing.
        let err = resolve_signer(&sig, Some(BUNDLE), &digest, None, Runtime::Wasm).unwrap_err();
        assert_eq!(
            (err.http_status_code(), err.code()),
            (403, "SIGNER_NOT_PERMITTED")
        );
        assert_eq!(
            err.message(),
            format!("signer not permitted to publish: issuer='{ISSUER}' subject='{SUBJECT}'")
        );

        // A policy naming another subject, or only another runtime.
        let other_subject = policy(vec![SignerRule::new(ISSUER, "repo:x", [Runtime::Wasm])]);
        assert_eq!(
            code(resolve_signer(
                &sig,
                Some(BUNDLE),
                &digest,
                Some(&other_subject),
                Runtime::Wasm
            )),
            (403, "SIGNER_NOT_PERMITTED".into())
        );
        assert_eq!(
            code(resolve_signer(
                &sig,
                Some(BUNDLE),
                &digest,
                Some(&exact),
                Runtime::Jvm
            )),
            (403, "SIGNER_NOT_PERMITTED".into())
        );

        // An exact match stores the certificate's identity.
        let signer = resolve_signer(&sig, Some(BUNDLE), &digest, Some(&exact), Runtime::Wasm)
            .unwrap()
            .unwrap();
        assert_eq!(
            (signer.issuer.as_str(), signer.subject.as_str()),
            (ISSUER, SUBJECT)
        );
    }

    #[test]
    fn signatures_off_store_no_signer_whatever_is_sent() {
        let digest = digest_of(ARTIFACT);
        for bundle in [None, Some("not even json"), Some(BUNDLE)] {
            assert_eq!(
                resolve_signer(&Signatures::Off, bundle, &digest, None, Runtime::Wasm).unwrap(),
                None
            );
        }
    }

    /// Java `FunctionArtifactUploadApiTest` U6/U7, the use case's half.
    #[tokio::test]
    async fn a_platform_ref_must_name_this_function_and_digest_and_exist() {
        let dir = std::env::temp_dir().join(format!(
            "fc-publish-ref-{}",
            crate::shared::tsid::generate_untyped()
        ));
        let store = FileArtifactBlobStore::new(dir.clone()).unwrap();
        let digest = digest_of(b"bytes");
        let src = dir.join("src.bin");
        std::fs::write(&src, b"bytes").unwrap();
        let reference = format!("platform://fnc_1/{}", digest.hex());
        let with: Option<&dyn ArtifactBlobStore> = Some(&store);
        let check = |store, r: &str, id: &str| {
            let (r, id, digest) = (r.to_string(), id.to_string(), digest.clone());
            async move { check_platform_ref(store, &r, &id, &digest).await }
        };

        // Any other scheme passes, with or without a store.
        assert!(check(None, "oci://r/a", "fnc_1").await.is_ok());
        // No store: 503.
        let err = check(None, &reference, "fnc_1").await.unwrap_err();
        assert_eq!(
            (err.http_status_code(), err.code()),
            (503, "ARTIFACT_STORE_NOT_CONFIGURED")
        );
        // Another function's id, another hex, a malformed ref: 422 mismatch.
        for bad in [
            format!("platform://fnc_2/{}", digest.hex()),
            format!("platform://fnc_1/{}", "0".repeat(64)),
            "platform://fnc_1".to_string(),
        ] {
            let err = check(with, &bad, "fnc_1").await.unwrap_err();
            assert_eq!(
                (err.http_status_code(), err.code()),
                (422, "ARTIFACT_REF_MISMATCH"),
                "{bad}"
            );
        }
        // Right function and digest, never uploaded: 422 not uploaded.
        let err = check(with, &reference, "fnc_1").await.unwrap_err();
        assert_eq!(
            (err.http_status_code(), err.code()),
            (422, "ARTIFACT_NOT_UPLOADED")
        );
        // Uploaded: passes.
        store.put("fnc_1", &digest, &src).await.unwrap();
        assert!(check(with, &reference, "fnc_1").await.is_ok());
        // A store failure is a 500, never a pass.
        let bad_id = format!("platform://fn.1/{}", digest.hex());
        let err = check(with, &bad_id, "fn.1").await.unwrap_err();
        assert_eq!(
            (err.http_status_code(), err.code()),
            (500, "ARTIFACT_STORE_ERROR")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn artifact_refs_follow_the_code_not_the_spec() {
        for ok in [
            "oci://registry/repo",
            "file:///a.wasm",
            "s3://bucket/key",
            "platform://fnc_1/abc",
        ] {
            assert!(validate_artifact_ref(ok).is_ok(), "{ok}");
        }
        for bad in [
            "oci://",
            "http://x",
            "OCI://x",
            "platform:/x",
            "oci://a\nb",
            " oci://x",
        ] {
            let err = validate_artifact_ref(bad).unwrap_err();
            assert_eq!(err.code(), "ARTIFACT_REF_INVALID", "{bad:?}");
            assert_eq!(
                err.message(),
                "artifactRef must start with oci://, file://, s3://, or platform://"
            );
        }
    }
}
