use crate::detect::Language;
use clap::Args;

#[derive(Debug, Clone, Args, Default)]
pub struct EcosystemArgs {
    /// crates.io / Cargo (alias: --rust)
    #[arg(long = "cargo", visible_alias = "rust", group = "eco")]
    pub cargo: bool,
    #[arg(long, group = "eco")]
    pub zig: bool,
    #[arg(long, group = "eco")]
    pub nim: bool,
    #[arg(long, group = "eco")]
    pub c: bool,
    #[arg(long, group = "eco")]
    pub cpp: bool,
    #[arg(long = "v", group = "eco")]
    pub vlang: bool,
    #[arg(long = "d", group = "eco")]
    pub dlang: bool,
    #[arg(long, group = "eco")]
    pub odin: bool,
    #[arg(long, group = "eco")]
    pub hare: bool,
    /// NuGet / C# (alias: --cs)
    #[arg(long = "csharp", visible_alias = "cs", group = "eco")]
    pub csharp: bool,
}

impl EcosystemArgs {
    pub fn language(&self) -> Option<Language> {
        if self.cargo {
            Some(Language::Rust)
        } else if self.zig {
            Some(Language::Zig)
        } else if self.nim {
            Some(Language::Nim)
        } else if self.c {
            Some(Language::C)
        } else if self.cpp {
            Some(Language::Cpp)
        } else if self.vlang {
            Some(Language::V)
        } else if self.dlang {
            Some(Language::D)
        } else if self.odin {
            Some(Language::Odin)
        } else if self.hare {
            Some(Language::Hare)
        } else if self.csharp {
            Some(Language::CSharp)
        } else {
            None
        }
    }

    pub fn ecosystem_key(&self) -> Option<&'static str> {
        self.language().map(Language::ecosystem)
    }
}
