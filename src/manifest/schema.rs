use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default = "default_schema")]
    pub schema_version: u32,
    pub host: Host,
    #[serde(default)]
    pub package: Option<PackageMeta>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
    #[serde(default)]
    pub expose: ExposeConfig,
    #[serde(default, rename = "tool", skip_serializing_if = "Option::is_none")]
    pub tool: Option<toml::Value>,
}

fn default_schema() -> u32 {
    1
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            schema_version: 1,
            host: Host {
                language: "rust".into(),
                manifest: None,
                root: None,
            },
            package: None,
            dependencies: BTreeMap::new(),
            expose: ExposeConfig::default(),
            tool: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Host {
    pub language: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub ecosystem: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_features: Option<bool>,
    /// Auto-expose via equilibrium-ffi (default true).
    #[serde(default = "default_true")]
    pub expose: bool,
    /// Nested expose options: [dependencies.name.expose_opts] or inline fields below.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expose_opts: Option<ExposeOpts>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExposeOpts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crate_types: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExposeConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Default dir for generated consumer imports / re-exports.
    /// Default `src/rig_bindings` (Rust re-exports). Override to `src/vendor` for Zig/C.
    #[serde(default = "default_expose_dir")]
    pub dir: String,
    #[serde(default = "default_cache")]
    pub cache: String,
    #[serde(default = "default_build_dir")]
    pub build_dir: String,
}

fn default_expose_dir() -> String {
    "src/rig_bindings".into()
}

fn default_cache() -> String {
    ".rig/cache".into()
}

fn default_build_dir() -> String {
    "target/rig".into()
}

impl Default for ExposeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: default_expose_dir(),
            cache: default_cache(),
            build_dir: default_build_dir(),
        }
    }
}
