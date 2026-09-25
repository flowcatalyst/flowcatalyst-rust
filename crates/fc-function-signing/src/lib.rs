//! Function artifact signatures (Java
//! `platform/function/artifact/{Signatures,SignaturesMode,Verification,
//! SignatureVerifier,TrustRoot}.java`, pinned at `0118cdca`): the Sigstore
//! bundle v0.3 verifier and the `FC_FN_SIGNATURES` policy, shared by the
//! platform (checked at publish, P4) and the function host (checked again
//! before a load).
//!
//! `fc-fnhost-core` re-exports this crate's [`digest`] and [`signature`]
//! modules under its own paths rather than carrying a copy, so a fix here is
//! the only fix either caller needs.
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
