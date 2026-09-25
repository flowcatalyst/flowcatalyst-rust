//! Java `PlatformArtifactRef`: `platform://<functionId>/<hex>`, what was
//! uploaded, scoped by function. Never a global content-addressed
//! namespace: that would let a tenant publish a digest another tenant
//! uploaded, and turn the upload route into an existence oracle.

use crate::function::Digest;

const PREFIX: &str = "platform://";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformArtifactRef {
    pub function_id: String,
    pub hex: String,
}

impl PlatformArtifactRef {
    /// The ref for an upload of `digest` under `function_id`.
    pub fn of(function_id: &str, digest: &Digest) -> PlatformArtifactRef {
        PlatformArtifactRef {
            function_id: function_id.to_string(),
            hex: digest.hex().to_string(),
        }
    }

    /// `None` for anything not shaped like `platform://x/y`: an empty id or
    /// hex, or an extra path segment. The parts are not format-checked
    /// here; every store call does that.
    pub fn parse(reference: &str) -> Option<PlatformArtifactRef> {
        let rest = reference.strip_prefix(PREFIX)?;
        let slash = rest.find('/')?;
        if slash == 0 || slash == rest.len() - 1 {
            return None;
        }
        let (function_id, hex) = (&rest[..slash], &rest[slash + 1..]);
        if hex.contains('/') {
            return None;
        }
        Some(PlatformArtifactRef {
            function_id: function_id.to_string(),
            hex: hex.to_string(),
        })
    }

    pub fn render(&self) -> String {
        format!("{PREFIX}{}/{}", self.function_id, self.hex)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_the_two_segment_shape() {
        let r = PlatformArtifactRef::parse("platform://fnc_1/abc").unwrap();
        assert_eq!((r.function_id.as_str(), r.hex.as_str()), ("fnc_1", "abc"));
        assert_eq!(r.render(), "platform://fnc_1/abc");
        for bad in [
            "platform://",
            "platform:///abc",
            "platform://fnc_1/",
            "platform://fnc_1",
            "platform://fnc_1/a/b",
            "oci://fnc_1/abc",
        ] {
            assert_eq!(PlatformArtifactRef::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn of_renders_the_upload_ref() {
        let d = Digest::parse(&format!("sha256:{}", "c".repeat(64))).unwrap();
        assert_eq!(
            PlatformArtifactRef::of("fnc_9", &d).render(),
            format!("platform://fnc_9/{}", "c".repeat(64))
        );
    }
}
