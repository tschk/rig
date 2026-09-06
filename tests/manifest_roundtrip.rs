use rig::manifest::{Manifest, load_manifest};
use std::path::PathBuf;

#[test]
fn example_rig_toml_parses() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let m = load_manifest(&root.join("rig.toml")).expect("parse example rig.toml");
    assert_eq!(m.schema_version, 1);
    assert_eq!(m.host.language, "rust");
    let rx4 = m.dependencies.get("rx4").expect("rx4");
    assert_eq!(rx4.ecosystem, "cargo");
    assert!(rx4.expose);
}

#[test]
fn default_manifest_roundtrip() {
    let m = Manifest::default();
    let s = toml::to_string_pretty(&m).unwrap();
    let back: Manifest = toml::from_str(&s).unwrap();
    assert_eq!(back.host.language, "rust");
}
