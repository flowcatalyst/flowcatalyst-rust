//! Every row of Java's shared `function-address-table.csv`
//! (`function-api/src/test/resources/`, `docs/spec/function-host-core.md` §1
//! in the Java repo), read the way Java's `FunctionAddressTest.sharedTable`
//! reads it.

use std::path::Path;

use fc_function_abi::FunctionAddress;

const TABLE: &str = "tests/data/function-address-table.csv";
const JAVA_TABLE: &str =
    "../../../flowcatalyst-javalin/function-api/src/test/resources/function-address-table.csv";

fn rows(text: &str) -> Vec<(String, bool)> {
    let mut rows = Vec::new();
    let mut saw_header = false;
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if !saw_header {
            saw_header = true;
            continue;
        }
        let open = line.find('"').expect("opening quote");
        let close = line.rfind('"').expect("closing quote");
        let raw = line[open + 1..close].to_string();
        let accepted = line[close + 1..].replace(',', "").trim() == "true";
        rows.push((raw, accepted));
    }
    rows
}

#[test]
fn shared_table() {
    let text = std::fs::read_to_string(TABLE).unwrap();
    let rows = rows(&text);
    assert_eq!(
        rows.len(),
        16,
        "function-address-table.csv must contain rows"
    );
    for (raw, accepted) in rows {
        match FunctionAddress::parse(&raw) {
            Ok(address) => {
                assert!(accepted, "{raw:?} should be refused");
                assert_eq!(address.render(), raw, "render(parse(raw)) == raw");
            }
            Err(_) => assert!(!accepted, "{raw:?} should be accepted"),
        }
    }
}

/// The copy must stay byte-identical to Java's (skipped when the Java
/// checkout is not beside this repo).
#[test]
fn copy_matches_java() {
    let java = Path::new(env!("CARGO_MANIFEST_DIR")).join(JAVA_TABLE);
    let Ok(java) = std::fs::read(&java) else {
        eprintln!("skipped: {} not found", java.display());
        return;
    };
    let ours = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(TABLE)).unwrap();
    assert!(
        ours == java,
        "tests/data/function-address-table.csv differs from Java's copy"
    );
}
