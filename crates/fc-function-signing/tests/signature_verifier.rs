//! Java's `SignatureVerifierTest` (server test sources, `0118cdca`), case for
//! case: the golden conformance bundle against the committed public-good
//! root, and every broken variant `TestSigstore` builds, each with the same
//! expected `Verification.Reason`.

mod support;

use chrono::{DateTime, Duration, TimeZone, Utc};
use fc_function_signing::digest::{Digest, SignerIdentity};
use fc_function_signing::signature::{Reason, SignatureVerifier, TrustRoot, Verification};
use serde_json::Value;
use support::sigstore::{self as ts, BundleOptions, LeafSpec};

const ENTRY_LOG_INDEX: i64 = 12345;

fn not_before() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 3, 19, 17, 26, 26).unwrap()
}
fn not_after() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 3, 19, 17, 36, 26).unwrap()
}
fn integrated_time() -> DateTime<Utc> {
    not_before() + Duration::seconds(60)
}
fn an_hour_before() -> DateTime<Utc> {
    not_before() - Duration::seconds(3600)
}

fn digest_bytes(text: &str) -> [u8; 32] {
    ts::sha256(text.as_bytes())
}
fn digest_of(bytes: [u8; 32]) -> Digest {
    Digest::from_sha256(&bytes)
}
fn root_for(eco: &ts::Ecosystem) -> TrustRoot {
    eco.trust_root_for(an_hour_before(), None, an_hour_before(), None)
}
fn verify(root: TrustRoot, bundle: &str, digest: [u8; 32]) -> Verification {
    SignatureVerifier::new(root).verify(Some(bundle), &digest_of(digest))
}

#[track_caller]
fn assert_rejected(result: Verification, expected: Reason) {
    match result {
        Verification::Rejected { reason, detail } => {
            assert_eq!(reason, expected, "rejected for the wrong reason: {detail}")
        }
        other => panic!("expected {expected:?}, got {other:?}"),
    }
}

#[track_caller]
fn assert_verified(result: &Verification) {
    assert!(
        matches!(result, Verification::Verified { .. }),
        "expected Verified, got {result:?}"
    );
}

fn edit(bundle: &str, f: impl FnOnce(&mut Value)) -> String {
    let mut root: Value = serde_json::from_str(bundle).unwrap();
    f(&mut root);
    serde_json::to_string(&root).unwrap()
}

fn entry(root: &mut Value) -> &mut Value {
    &mut root["verificationMaterial"]["tlogEntries"][0]
}

fn valid() -> (ts::Ecosystem, [u8; 32], String) {
    let eco = ts::build(&LeafSpec::valid(not_before(), not_after()));
    let digest = digest_bytes("hello artifact");
    let bundle = ts::valid_bundle_json(&eco, digest, integrated_time(), ENTRY_LOG_INDEX);
    (eco, digest, bundle)
}

// ---- golden fixture ----

#[test]
fn golden_fixture_verifies_with_the_real_issuer_and_subject() {
    let bundle = include_str!("fixtures/sigstore/happy-path-v0.3.sigstore.json");
    let artifact = include_bytes!("fixtures/sigstore/artifact.txt");
    let result = SignatureVerifier::new(TrustRoot::sigstore_public_good())
        .verify(Some(bundle), &digest_of(ts::sha256(artifact)));
    assert_eq!(
        result,
        Verification::Verified {
            signer: SignerIdentity::new(
                "https://token.actions.githubusercontent.com",
                "https://github.com/sigstore-conformance/extremely-dangerous-public-oidc-beacon/\
                 .github/workflows/extremely-dangerous-oidc-beacon.yml@refs/heads/main"
            ),
            signed_at: Utc.timestamp_opt(1_710_869_186, 0).unwrap(),
        }
    );
}

#[test]
fn golden_fixture_against_a_different_artifact_is_a_digest_mismatch() {
    let bundle = include_str!("fixtures/sigstore/happy-path-v0.3.sigstore.json");
    let result = SignatureVerifier::new(TrustRoot::sigstore_public_good())
        .verify(Some(bundle), &digest_of(digest_bytes("not a.txt")));
    assert_rejected(result, Reason::DigestMismatch);
}

// ---- round trip; integratedTime, not now(), governs validity ----

#[test]
fn valid_bundle_verifies_even_though_the_leaf_expired_long_ago() {
    assert!(Utc::now() > not_after() + Duration::days(365));
    let (eco, digest, bundle) = valid();
    assert_verified(&verify(root_for(&eco), &bundle, digest));
}

// ---- one broken variant per step ----

#[test]
fn step1_wrong_digest_is_rejected() {
    let eco = ts::build(&LeafSpec::valid(not_before(), not_after()));
    let bundle = ts::valid_bundle_json(
        &eco,
        digest_bytes("the real artifact"),
        integrated_time(),
        ENTRY_LOG_INDEX,
    );
    assert_rejected(
        verify(
            root_for(&eco),
            &bundle,
            digest_bytes("a different artifact"),
        ),
        Reason::DigestMismatch,
    );
}

#[test]
fn step2_log_from_an_unpinned_key_is_rejected() {
    let (_eco, digest, bundle) = valid();
    let other = ts::build(&LeafSpec::valid(not_before(), not_after()));
    assert_rejected(
        verify(root_for(&other), &bundle, digest),
        Reason::TlogUnknownLog,
    );
}

#[test]
fn step3_log_entry_spliced_from_a_different_digest_is_rejected() {
    let eco = ts::build(&LeafSpec::valid(not_before(), not_after()));
    let digest_a = digest_bytes("artifact A");
    let digest_b = digest_bytes("artifact B, a completely different one");
    let bundle_a = ts::valid_bundle_json(&eco, digest_a, integrated_time(), ENTRY_LOG_INDEX);
    let bundle_b = ts::valid_bundle_json(&eco, digest_b, integrated_time(), ENTRY_LOG_INDEX + 1);
    let entry_b: Value = serde_json::from_str::<Value>(&bundle_b).unwrap()["verificationMaterial"]
        ["tlogEntries"][0]
        .clone();
    let spliced = edit(&bundle_a, |root| *entry(root) = entry_b);
    assert_rejected(
        verify(root_for(&eco), &spliced, digest_a),
        Reason::TlogEntryMismatch,
    );
}

#[test]
fn step3_hash_value_alone_mismatch_is_rejected() {
    let eco = ts::build(&LeafSpec::valid(not_before(), not_after()));
    let digest = digest_bytes("hello artifact");
    let bundle = ts::bundle_json(
        &eco,
        digest,
        integrated_time(),
        ENTRY_LOG_INDEX,
        BundleOptions {
            rekor_hash_value_hex: Some("00".repeat(32)),
            ..Default::default()
        },
    );
    assert_rejected(
        verify(root_for(&eco), &bundle, digest),
        Reason::TlogEntryMismatch,
    );
}

#[test]
fn step4_corrupted_signed_entry_timestamp_is_rejected() {
    let (eco, digest, bundle) = valid();
    let corrupted = edit(&bundle, |root| {
        entry(root)["inclusionPromise"]["signedEntryTimestamp"] =
            ts::b64(&[1, 2, 3, 4, 5, 6, 7, 8]).into()
    });
    assert_rejected(
        verify(root_for(&eco), &corrupted, digest),
        Reason::TlogPromiseInvalid,
    );
}

#[test]
fn step5_corrupted_inclusion_proof_root_hash_is_rejected() {
    let (eco, digest, bundle) = valid();
    let corrupted = edit(&bundle, |root| {
        entry(root)["inclusionProof"]["rootHash"] = ts::b64(&[0u8; 32]).into()
    });
    assert_rejected(
        verify(root_for(&eco), &corrupted, digest),
        Reason::TlogInclusionInvalid,
    );
}

#[test]
fn step6_certificate_not_chaining_to_a_pinned_ca_is_rejected() {
    let (eco, digest, bundle) = valid();
    let other = ts::build(&LeafSpec::valid(not_before(), not_after()));
    let mismatched = TrustRoot {
        cas: root_for(&other).cas,
        tlogs: root_for(&eco).tlogs,
    };
    assert_rejected(
        verify(mismatched, &bundle, digest),
        Reason::UntrustedCertificate,
    );
}

#[test]
fn step7_signature_over_the_wrong_bytes_is_rejected() {
    let eco = ts::build(&LeafSpec::valid(not_before(), not_after()));
    let declared = digest_bytes("hello artifact");
    let bundle = ts::bundle_json(
        &eco,
        declared,
        integrated_time(),
        ENTRY_LOG_INDEX,
        BundleOptions {
            sign_over_digest: Some(digest_bytes(
                "a value the leaf key never signed the declared digest for",
            )),
            ..Default::default()
        },
    );
    assert_rejected(
        verify(root_for(&eco), &bundle, declared),
        Reason::BadSignature,
    );
}

#[test]
fn step8_missing_san_is_rejected() {
    let eco = ts::build(&LeafSpec::valid(not_before(), not_after()).without_san());
    let digest = digest_bytes("hello artifact");
    let bundle = ts::valid_bundle_json(&eco, digest, integrated_time(), ENTRY_LOG_INDEX);
    assert_rejected(
        verify(root_for(&eco), &bundle, digest),
        Reason::IdentityMissing,
    );
}

#[test]
fn non_ec_leaf_key_is_unsupported() {
    let eco = ts::build(&LeafSpec::valid(not_before(), not_after()).with_rsa_key());
    let digest = digest_bytes("hello artifact");
    let bundle = ts::valid_bundle_json(&eco, digest, integrated_time(), ENTRY_LOG_INDEX);
    assert_rejected(
        verify(root_for(&eco), &bundle, digest),
        Reason::UnsupportedBundle,
    );
}

// ---- step 3, isolated bindings ----

#[test]
fn step3_signature_content_alone_mismatch_is_rejected() {
    let eco = ts::build(&LeafSpec::valid(not_before(), not_after()));
    let digest = digest_bytes("hello artifact");
    let bundle = ts::bundle_json(
        &eco,
        digest,
        integrated_time(),
        ENTRY_LOG_INDEX,
        BundleOptions {
            entry_signature_override: Some(ts::random_bytes(64)),
            ..Default::default()
        },
    );
    assert_rejected(
        verify(root_for(&eco), &bundle, digest),
        Reason::TlogEntryMismatch,
    );
}

#[test]
fn step3_public_key_alone_mismatch_is_rejected() {
    let eco = ts::build(&LeafSpec::valid(not_before(), not_after()));
    let digest = digest_bytes("hello artifact");
    let bundle = ts::bundle_json(
        &eco,
        digest,
        integrated_time(),
        ENTRY_LOG_INDEX,
        BundleOptions {
            entry_public_key_cert_der_override: Some(eco.root_der.clone()),
            ..Default::default()
        },
    );
    assert_rejected(
        verify(root_for(&eco), &bundle, digest),
        Reason::TlogEntryMismatch,
    );
}

// ---- step 4: the SET covers every field it authenticates ----

#[test]
fn set_invalid_when_integrated_time_altered_after_signing() {
    let (eco, digest, bundle) = valid();
    let corrupted = edit(&bundle, |root| {
        entry(root)["integratedTime"] = (integrated_time().timestamp() + 1).to_string().into()
    });
    assert_rejected(
        verify(root_for(&eco), &corrupted, digest),
        Reason::TlogPromiseInvalid,
    );
}

#[test]
fn set_invalid_when_entry_log_index_altered_after_signing() {
    let (eco, digest, bundle) = valid();
    let corrupted = edit(&bundle, |root| {
        entry(root)["logIndex"] = (ENTRY_LOG_INDEX + 1).to_string().into()
    });
    assert_rejected(
        verify(root_for(&eco), &corrupted, digest),
        Reason::TlogPromiseInvalid,
    );
}

// ---- step 5: the checkpoint must describe the proof's tree ----

fn checkpoint_variant(options: BundleOptions) -> Verification {
    let eco = ts::build(&LeafSpec::valid(not_before(), not_after()));
    let digest = digest_bytes("hello artifact");
    let bundle = ts::bundle_json(&eco, digest, integrated_time(), ENTRY_LOG_INDEX, options);
    verify(root_for(&eco), &bundle, digest)
}

#[test]
fn checkpoint_validly_signed_but_for_a_different_root_is_rejected() {
    let result = checkpoint_variant(BundleOptions {
        checkpoint_root_override: Some(ts::random_bytes(32)),
        ..Default::default()
    });
    assert_rejected(result, Reason::TlogCheckpointInvalid);
}

#[test]
fn checkpoint_validly_signed_but_for_a_different_size_is_rejected() {
    let result = checkpoint_variant(BundleOptions {
        checkpoint_size_override: Some(2),
        ..Default::default()
    });
    assert_rejected(result, Reason::TlogCheckpointInvalid);
}

#[test]
fn checkpoint_correct_content_but_signed_by_an_unpinned_key_is_rejected() {
    let result = checkpoint_variant(BundleOptions {
        checkpoint_signing_key: Some(ts::fresh_ec_key()),
        ..Default::default()
    });
    assert_rejected(result, Reason::TlogCheckpointInvalid);
}

fn tree_bundle() -> (ts::Ecosystem, [u8; 32], String) {
    let eco = ts::build(&LeafSpec::valid(not_before(), not_after()));
    let digest = digest_bytes("hello artifact");
    let bundle = ts::bundle_json(
        &eco,
        digest,
        integrated_time(),
        ENTRY_LOG_INDEX,
        BundleOptions {
            tree_size: Some(6),
            leaf_index: Some(2),
            ..Default::default()
        },
    );
    (eco, digest, bundle)
}

#[test]
fn multi_leaf_inclusion_proof_verifies_as_a_baseline() {
    let (eco, digest, bundle) = tree_bundle();
    assert_verified(&verify(root_for(&eco), &bundle, digest));
}

#[test]
fn multi_leaf_inclusion_proof_with_a_wrong_log_index_is_rejected() {
    let (eco, digest, bundle) = tree_bundle();
    let corrupted = edit(&bundle, |root| {
        entry(root)["inclusionProof"]["logIndex"] = "3".into()
    });
    assert_rejected(
        verify(root_for(&eco), &corrupted, digest),
        Reason::TlogInclusionInvalid,
    );
}

#[test]
fn multi_leaf_inclusion_proof_with_a_removed_hash_is_rejected() {
    let (eco, digest, bundle) = tree_bundle();
    let corrupted = edit(&bundle, |root| {
        let hashes = entry(root)["inclusionProof"]["hashes"]
            .as_array_mut()
            .unwrap();
        assert!(
            hashes.len() > 1,
            "a 6-leaf tree's audit path at index 2 is non-trivial"
        );
        hashes.pop();
    });
    assert_rejected(
        verify(root_for(&eco), &corrupted, digest),
        Reason::TlogInclusionInvalid,
    );
}

// ---- step 6: EKU and each window separately ----

#[test]
fn step6_no_code_signing_eku_is_rejected() {
    let eco = ts::build(&LeafSpec::valid(not_before(), not_after()).without_eku());
    let digest = digest_bytes("hello artifact");
    let bundle = ts::valid_bundle_json(&eco, digest, integrated_time(), ENTRY_LOG_INDEX);
    assert_rejected(
        verify(root_for(&eco), &bundle, digest),
        Reason::NotACodeSigningCertificate,
    );
}

#[test]
fn step6_ca_window_not_covering_integrated_time_is_untrusted_certificate() {
    let (eco, digest, bundle) = valid();
    let root = eco.trust_root_for(
        integrated_time() + Duration::seconds(60),
        None,
        an_hour_before(),
        None,
    );
    assert_rejected(verify(root, &bundle, digest), Reason::UntrustedCertificate);
}

#[test]
fn step6_log_key_window_not_covering_integrated_time_is_unknown_log() {
    let (eco, digest, bundle) = valid();
    let root = eco.trust_root_for(
        an_hour_before(),
        None,
        integrated_time() + Duration::seconds(60),
        None,
    );
    assert_rejected(verify(root, &bundle, digest), Reason::TlogUnknownLog);
}

#[test]
fn integrated_time_outside_leaf_validity_is_certificate_not_valid_at_signing() {
    let eco = ts::build(&LeafSpec::valid(
        not_before(),
        not_before() + Duration::days(1),
    ));
    let digest = digest_bytes("hello artifact");
    let bundle = ts::valid_bundle_json(
        &eco,
        digest,
        not_before() + Duration::days(3),
        ENTRY_LOG_INDEX,
    );
    assert_rejected(
        verify(root_for(&eco), &bundle, digest),
        Reason::CertificateNotValidAtSigning,
    );
}

// ---- step 8, the remaining clauses ----

#[test]
fn step8_two_sans_is_rejected() {
    let spec = LeafSpec::valid(not_before(), not_after()).with_sans(vec![
        ("uri", "https://one.example/workflow.yml".into()),
        ("uri", "https://two.example/workflow.yml".into()),
    ]);
    let eco = ts::build(&spec);
    let digest = digest_bytes("hello artifact");
    let bundle = ts::valid_bundle_json(&eco, digest, integrated_time(), ENTRY_LOG_INDEX);
    assert_rejected(
        verify(root_for(&eco), &bundle, digest),
        Reason::IdentityMissing,
    );
}

#[test]
fn step8_issuer_extension_absent_is_rejected() {
    let eco = ts::build(&LeafSpec::valid(not_before(), not_after()).without_issuer());
    let digest = digest_bytes("hello artifact");
    let bundle = ts::valid_bundle_json(&eco, digest, integrated_time(), ENTRY_LOG_INDEX);
    assert_rejected(
        verify(root_for(&eco), &bundle, digest),
        Reason::IdentityMissing,
    );
}

#[test]
fn step8_deprecated_issuer_extension_alone_is_read() {
    let spec = LeafSpec::valid(not_before(), not_after()).with_issuer(
        ts::ISSUER_OID_DEPRECATED,
        "https://deprecated.example/issuer",
    );
    let eco = ts::build(&spec);
    let digest = digest_bytes("hello artifact");
    let bundle = ts::valid_bundle_json(&eco, digest, integrated_time(), ENTRY_LOG_INDEX);
    match verify(root_for(&eco), &bundle, digest) {
        Verification::Verified { signer, .. } => {
            assert_eq!(signer.issuer, "https://deprecated.example/issuer")
        }
        other => panic!("expected Verified, got {other:?}"),
    }
}

#[test]
fn an_rfc822_san_is_an_identity() {
    let eco = ts::build(
        &LeafSpec::valid(not_before(), not_after()).with_san("email", "dev@example.test"),
    );
    let digest = digest_bytes("hello artifact");
    let bundle = ts::valid_bundle_json(&eco, digest, integrated_time(), ENTRY_LOG_INDEX);
    match verify(root_for(&eco), &bundle, digest) {
        Verification::Verified { signer, .. } => assert_eq!(signer.subject, "dev@example.test"),
        other => panic!("expected Verified, got {other:?}"),
    }
}

// ---- bundle shapes this verifier does not read ----

#[test]
fn old_media_type_is_unsupported() {
    let (eco, digest, bundle) = valid();
    let corrupted = edit(&bundle, |root| {
        root["mediaType"] = "application/vnd.dev.sigstore.bundle+json;version=0.1".into()
    });
    assert_rejected(
        verify(root_for(&eco), &corrupted, digest),
        Reason::UnsupportedBundle,
    );
}

#[test]
fn dsse_envelope_bundle_is_unsupported() {
    let (eco, digest, bundle) = valid();
    let corrupted = edit(&bundle, |root| {
        root["dsseEnvelope"] = serde_json::json!({"payload": "eyJ9"})
    });
    assert_rejected(
        verify(root_for(&eco), &corrupted, digest),
        Reason::UnsupportedBundle,
    );
}

#[test]
fn certificate_chain_instead_of_single_leaf_is_unsupported() {
    let (eco, digest, bundle) = valid();
    let corrupted = edit(&bundle, |root| {
        root["verificationMaterial"]["x509CertificateChain"] = serde_json::json!([])
    });
    assert_rejected(
        verify(root_for(&eco), &corrupted, digest),
        Reason::UnsupportedBundle,
    );
}

#[test]
fn two_tlog_entries_is_unsupported() {
    let (eco, digest, bundle) = valid();
    let corrupted = edit(&bundle, |root| {
        let entries = root["verificationMaterial"]["tlogEntries"]
            .as_array_mut()
            .unwrap();
        entries.push(entries[0].clone());
    });
    assert_rejected(
        verify(root_for(&eco), &corrupted, digest),
        Reason::UnsupportedBundle,
    );
}

#[test]
fn tlog_entry_kind_intoto_is_unsupported() {
    let (eco, digest, bundle) = valid();
    let corrupted = edit(&bundle, |root| {
        entry(root)["kindVersion"]["kind"] = "intoto".into()
    });
    assert_rejected(
        verify(root_for(&eco), &corrupted, digest),
        Reason::UnsupportedBundle,
    );
}

#[test]
fn missing_inclusion_promise_is_tlog_missing() {
    let (eco, digest, bundle) = valid();
    let corrupted = edit(&bundle, |root| {
        entry(root)
            .as_object_mut()
            .unwrap()
            .remove("inclusionPromise");
    });
    assert_rejected(
        verify(root_for(&eco), &corrupted, digest),
        Reason::TlogMissing,
    );
}

// ---- never throws ----

#[test]
fn never_panics_on_fuzz_input() {
    let eco = ts::build(&LeafSpec::valid(not_before(), not_after()));
    let verifier = SignatureVerifier::new(root_for(&eco));
    let digest = digest_of(digest_bytes("whatever"));
    let deep = "{".repeat(10 * 1024 * 1024);
    for input in [
        None,
        Some(""),
        Some("[]"),
        Some("{\"mediaType\":"),
        Some(deep.as_str()),
    ] {
        assert!(matches!(
            verifier.verify(input, &digest),
            Verification::Rejected { .. }
        ));
    }
}

#[test]
fn never_panics_on_a_truncated_der_extension() {
    let spec = LeafSpec::valid(not_before(), not_after()).with_raw_issuer_extension(vec![0x0c]);
    let eco = ts::build(&spec);
    let digest = digest_bytes("hello artifact");
    let bundle = ts::valid_bundle_json(&eco, digest, integrated_time(), ENTRY_LOG_INDEX);
    assert_rejected(
        verify(root_for(&eco), &bundle, digest),
        Reason::MalformedBundle,
    );
}

// ---- identity is reported verbatim ----

#[test]
fn identity_is_reported_verbatim_including_a_trailing_slash() {
    let subject = "https://example.test/workflow.yml/";
    let eco = ts::build(&LeafSpec::valid(not_before(), not_after()).with_san("uri", subject));
    let digest = digest_bytes("hello artifact");
    let bundle = ts::valid_bundle_json(&eco, digest, integrated_time(), ENTRY_LOG_INDEX);
    match verify(root_for(&eco), &bundle, digest) {
        Verification::Verified { signer, .. } => assert_eq!(signer.subject, subject),
        other => panic!("expected Verified, got {other:?}"),
    }
}
