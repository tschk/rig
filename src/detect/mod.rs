use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
    Zig,
    Nim,
    C,
    Cpp,
    V,
    D,
    Odin,
    Hare,
    CSharp,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Zig => "zig",
            Self::Nim => "nim",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::V => "v",
            Self::D => "d",
            Self::Odin => "odin",
            Self::Hare => "hare",
            Self::CSharp => "csharp",
        }
    }

    pub fn ecosystem(self) -> &'static str {
        match self {
            Self::Rust => "cargo",
            Self::Zig => "zig",
            Self::Nim => "nim",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::V => "v",
            Self::D => "d",
            Self::Odin => "odin",
            Self::Hare => "hare",
            Self::CSharp => "csharp",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "rust" | "cargo" => Self::Rust,
            "zig" => Self::Zig,
            "nim" => Self::Nim,
            "c" => Self::C,
            "cpp" | "c++" | "cplusplus" => Self::Cpp,
            "v" | "vlang" => Self::V,
            "d" | "dlang" => Self::D,
            "odin" => Self::Odin,
            "hare" => Self::Hare,
            "csharp" | "cs" | "c#" => Self::CSharp,
            other => bail!("unknown language: {other}"),
        })
    }

    pub fn all() -> &'static [Language] {
        &[
            Self::Rust,
            Self::Zig,
            Self::Nim,
            Self::C,
            Self::Cpp,
            Self::V,
            Self::D,
            Self::Odin,
            Self::Hare,
            Self::CSharp,
        ]
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct DetectedHost {
    pub language: Language,
    pub root: PathBuf,
    pub marker: Option<PathBuf>,
}

pub fn detect_host(start: &Path) -> Result<DetectedHost> {
    if let Ok(env) = std::env::var("RIG_HOST_LANG")
        && !env.is_empty()
    {
        return Ok(DetectedHost {
            language: Language::parse(&env)?,
            root: start.to_path_buf(),
            marker: None,
        });
    }

    let root = find_project_root(start);
    if let Some((lang, marker)) = detect_markers(&root) {
        return Ok(DetectedHost {
            language: lang,
            root,
            marker: Some(marker),
        });
    }
    if let Some(lang) = census_extensions(&root) {
        return Ok(DetectedHost {
            language: lang,
            root,
            marker: None,
        });
    }
    bail!(
        "could not detect host language in {} — pass --host or set RIG_HOST_LANG",
        root.display()
    )
}

fn find_project_root(start: &Path) -> PathBuf {
    let mut cur = start.to_path_buf();
    loop {
        if cur.join(".git").exists() || cur.join("rig.toml").exists() {
            return cur;
        }
        if !cur.pop() {
            return start.to_path_buf();
        }
    }
}

fn detect_markers(root: &Path) -> Option<(Language, PathBuf)> {
    let checks: &[(&str, Language)] = &[
        ("Cargo.toml", Language::Rust),
        ("build.zig", Language::Zig),
        ("build.zig.zon", Language::Zig),
        ("dub.json", Language::D),
        ("dub.sdl", Language::D),
        ("v.mod", Language::V),
        ("ols.json", Language::Odin),
        ("nimble.paths", Language::Nim),
    ];
    for (name, lang) in checks {
        let p = root.join(name);
        if p.exists() {
            return Some((*lang, p));
        }
    }
    // glob-ish markers
    if let Ok(rd) = std::fs::read_dir(root) {
        for ent in rd.flatten() {
            let name = ent.file_name();
            let s = name.to_string_lossy();
            if s.ends_with(".nimble") {
                return Some((Language::Nim, ent.path()));
            }
            if s.ends_with(".csproj") || s.ends_with(".sln") {
                return Some((Language::CSharp, ent.path()));
            }
            if s.ends_with(".ha") {
                return Some((Language::Hare, ent.path()));
            }
        }
    }
    // C / C++ via CMake/meson/Makefile + extension dominance
    let has_cmake = root.join("CMakeLists.txt").exists();
    let has_meson = root.join("meson.build").exists();
    let has_make = root.join("Makefile").exists() || root.join("makefile").exists();
    if has_cmake || has_meson || has_make {
        let (c, cpp) = count_c_family(root);
        if cpp > c {
            return Some((
                Language::Cpp,
                root.join(if has_cmake {
                    "CMakeLists.txt"
                } else if has_meson {
                    "meson.build"
                } else {
                    "Makefile"
                }),
            ));
        }
        if c > 0 || has_cmake || has_meson || has_make {
            return Some((
                Language::C,
                root.join(if has_cmake {
                    "CMakeLists.txt"
                } else if has_meson {
                    "meson.build"
                } else {
                    "Makefile"
                }),
            ));
        }
    }
    None
}

fn count_c_family(root: &Path) -> (usize, usize) {
    let mut c = 0usize;
    let mut cpp = 0usize;
    for ent in WalkDir::new(root)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = ent.path();
        if should_skip(path) {
            continue;
        }
        match path.extension().and_then(|e| e.to_str()) {
            Some("c") | Some("h") => c += 1,
            Some("cpp") | Some("cxx") | Some("cc") | Some("hpp") | Some("hxx") => cpp += 1,
            _ => {}
        }
    }
    (c, cpp)
}

fn census_extensions(root: &Path) -> Option<Language> {
    let mut counts = [0usize; 10];
    for ent in WalkDir::new(root)
        .max_depth(4)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = ent.path();
        if should_skip(path) {
            continue;
        }
        let idx = match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => 0,
            Some("zig") => 1,
            Some("nim") => 2,
            Some("c") | Some("h") => 3,
            Some("cpp") | Some("cxx") | Some("cc") | Some("hpp") => 4,
            Some("v") => 5,
            Some("d") => 6,
            Some("odin") => 7,
            Some("ha") => 8,
            Some("cs") => 9,
            _ => continue,
        };
        counts[idx] += 1;
    }
    let (idx, &n) = counts.iter().enumerate().max_by_key(|(_, n)| *n)?;
    if n == 0 {
        return None;
    }
    Some(Language::all()[idx])
}

fn should_skip(path: &Path) -> bool {
    path.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some("target") | Some(".git") | Some("node_modules") | Some(".rig") | Some("vendor")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn detects_cargo() {
        let d = tempdir().unwrap();
        fs::write(d.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        let h = detect_host(d.path()).unwrap();
        assert_eq!(h.language, Language::Rust);
    }

    #[test]
    fn detects_zig() {
        let d = tempdir().unwrap();
        fs::write(d.path().join("build.zig"), "").unwrap();
        let h = detect_host(d.path()).unwrap();
        assert_eq!(h.language, Language::Zig);
    }

    #[test]
    fn parse_aliases() {
        assert_eq!(Language::parse("cargo").unwrap(), Language::Rust);
        assert_eq!(Language::parse("cs").unwrap(), Language::CSharp);
    }
}
