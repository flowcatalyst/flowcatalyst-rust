//! Function artifact signatures (Java
//! `platform/function/artifact/{Signatures,SignaturesMode,Verification,
//! SignatureVerifier,TrustRoot}.java`, pinned at `0118cdca`): the Sigstore
//! bundle v0.3 verifier and the `FC_FN_SIGNATURES` policy, shared by the
//! platform (checked at publish, P4) and the function host (checked again
//! before a load).
//!
//! **Temporary duplication.** This crate is a verbatim copy of
//! `fc-fnhost-core`'s `signature` module, its `Digest`/`SignerIdentity` and
//! the JDK helpers it uses, together with its tests and Java's fixtures.
//! It was split out while `fc-fnhost-core` was being edited concurrently;
//! the host still carries its own copy. A later dedupe pass switches
//! `fc-fnhost-core` to depend on this crate and deletes the copy there. Until
//! then, a fix to either copy must be made to both.
//!
//! | Module | Java |
//! |---|---|
//! | [`digest`] | `platform/function/{Digest,SignerIdentity}.java` |
//! | [`signature`] | `platform/function/artifact/{Signatures,SignaturesMode,Verification,SignatureVerifier,TrustRoot}.java` |

pub mod digest;
mod java;
pub mod signature;

pub use digest::{Digest, SignerIdentity};
pub use signature::{
    CertificateAuthority, Reason, SignatureVerifier, Signatures, SignaturesMode, TransparencyLog,
    TrustRoot, Verification,
};
