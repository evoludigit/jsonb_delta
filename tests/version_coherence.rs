//! Guards the coherence between the crate version, the extension control
//! file, and the shipped SQL script. Issue #14 was the crate advancing to
//! 0.2.0 while the control file kept installing 0.1.0.

use std::path::Path;

/// Parse `default_version` out of an extension control file.
fn control_default_version(control: &str) -> Option<String> {
    control.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if key.trim() != "default_version" {
            return None;
        }
        Some(value.trim().trim_matches('\'').to_string())
    })
}

fn control_file() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("jsonb_delta.control");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

#[test]
fn control_default_version_matches_crate_version() {
    let control = control_file();
    let default_version =
        control_default_version(&control).expect("jsonb_delta.control has no default_version line");
    assert_eq!(
        default_version,
        env!("CARGO_PKG_VERSION"),
        "jsonb_delta.control default_version ({default_version}) disagrees with \
         Cargo.toml version ({}) — users would install an extension labelled \
         with the wrong version",
        env!("CARGO_PKG_VERSION"),
    );
}

#[test]
fn sql_script_for_crate_version_is_shipped() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("sql")
        .join(format!("jsonb_delta--{}.sql", env!("CARGO_PKG_VERSION")));
    assert!(
        script.is_file(),
        "{} is missing — regenerate with `just schema`",
        script.display(),
    );
}

#[test]
fn parses_quoted_default_version() {
    assert_eq!(
        control_default_version("comment = 'x'\ndefault_version = '0.9.9'\n"),
        Some("0.9.9".to_string()),
    );
    assert_eq!(control_default_version("comment = 'x'\n"), None);
}
