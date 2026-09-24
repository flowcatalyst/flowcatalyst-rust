//! Parses `/functions/{address}[:{version}]/{path}` (Java
//! `fnhost/http/RoutePath.java`). The raw path is matched undecoded; an
//! address never contains `:`, so the split on the first `:` is
//! unambiguous.

use fc_function_abi::FunctionAddress;

const PREFIX: &str = "/functions/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RoutePath {
    Matched {
        address: FunctionAddress,
        version: Option<i32>,
        /// `/` + whatever followed the address segment.
        function_path: String,
    },
    /// Not under `/functions/`: 404 `NOT_FOUND`.
    NotFunctionsRoute,
    /// 400 `ADDRESS_INVALID`.
    AddressInvalid,
    /// 400 `VERSION_INVALID`: present but not a positive `int`.
    VersionInvalid,
}

impl RoutePath {
    pub fn parse(raw_path: &str) -> Self {
        let Some(after_prefix) = raw_path.strip_prefix(PREFIX) else {
            return Self::NotFunctionsRoute;
        };
        let (address_and_version, rest) =
            after_prefix.split_once('/').unwrap_or((after_prefix, ""));
        if address_and_version.is_empty() {
            return Self::AddressInvalid;
        }
        let (address_raw, version) = match address_and_version.split_once(':') {
            Some((address, version_raw)) => match parse_java_int(version_raw) {
                Some(version) if version > 0 => (address, Some(version)),
                _ => return Self::VersionInvalid,
            },
            None => (address_and_version, None),
        };
        match FunctionAddress::parse(address_raw) {
            Ok(address) => Self::Matched {
                address,
                version,
                function_path: format!("/{rest}"),
            },
            Err(_) => Self::AddressInvalid,
        }
    }
}

/// `Integer.parseInt`: an optional sign, then ASCII digits, within `i32`.
fn parse_java_int(raw: &str) -> Option<i32> {
    let digits = raw.strip_prefix(['+', '-']).unwrap_or(raw);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matched(raw: &str) -> (String, Option<i32>, String) {
        match RoutePath::parse(raw) {
            RoutePath::Matched {
                address,
                version,
                function_path,
            } => (address.render(), version, function_path),
            other => panic!("{raw}: {other:?}"),
        }
    }

    #[test]
    fn parses_as_java() {
        assert_eq!(
            matched("/functions/a.b.c/echo/42"),
            ("a.b.c".into(), None, "/echo/42".into())
        );
        assert_eq!(
            matched("/functions/a.b.c"),
            ("a.b.c".into(), None, "/".into())
        );
        assert_eq!(
            matched("/functions/a.b.c/"),
            ("a.b.c".into(), None, "/".into())
        );
        assert_eq!(
            matched("/functions/a.b.c:12/x/y"),
            ("a.b.c".into(), Some(12), "/x/y".into())
        );
        assert_eq!(matched("/functions/a.b.c:+3/x").1, Some(3));
        assert_eq!(RoutePath::parse("/other"), RoutePath::NotFunctionsRoute);
        assert_eq!(RoutePath::parse("/functions"), RoutePath::NotFunctionsRoute);
        assert_eq!(RoutePath::parse("/functions/"), RoutePath::AddressInvalid);
        assert_eq!(RoutePath::parse("/functions//x"), RoutePath::AddressInvalid);
        assert_eq!(
            RoutePath::parse("/functions/A.b.c/x"),
            RoutePath::AddressInvalid
        );
        assert_eq!(
            RoutePath::parse("/functions/a.b/x"),
            RoutePath::AddressInvalid
        );
        for bad in ["0", "-1", "x", "", "99999999999", "1.0"] {
            assert_eq!(
                RoutePath::parse(&format!("/functions/a.b.c:{bad}/x")),
                RoutePath::VersionInvalid,
                "{bad}"
            );
        }
        // the version is checked before the address, as Java
        assert_eq!(
            RoutePath::parse("/functions/BAD:0/x"),
            RoutePath::VersionInvalid
        );
    }
}
