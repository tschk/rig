pub mod commands;
pub mod ecosystem;
pub mod globals;

pub use commands::*;
pub use ecosystem::EcosystemArgs;
pub use globals::Globals;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "rig",
    version,
    about = "Cross-lang native deps via equilibrium-ffi",
    long_about = "Project-local polyglot native dependency manager.\nDetect host language, resolve packages, track them in rig.toml, and auto-expose via equilibrium-ffi."
)]
pub struct Cli {
    #[command(flatten)]
    pub globals: Globals,
    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create rig.toml in the current project
    Init(commands::init::InitArgs),

    /// Add packages and auto-expose via equilibrium-ffi
    #[command(visible_aliases = ["i", "install"])]
    Add(commands::add::AddArgs),

    /// Remove packages and refresh expose
    #[command(visible_aliases = ["rm", "ui", "uninstall"])]
    Remove(commands::remove::RemoveArgs),

    /// Upgrade packages (all if omitted)
    #[command(visible_aliases = ["up"])]
    Upgrade(commands::upgrade::UpgradeArgs),

    /// List dependencies from rig.toml / lock
    #[command(visible_aliases = ["ls"])]
    List(commands::list::ListArgs),

    /// Search ecosystem registries
    #[command(visible_aliases = ["s", "find"])]
    Search(commands::search::SearchArgs),

    /// Show resolved package metadata
    #[command(visible_aliases = ["show"])]
    Info(commands::info::InfoArgs),

    /// List deps with newer versions available
    Outdated(commands::outdated::OutdatedArgs),

    /// Regenerate rig.lock from rig.toml
    Lock(commands::lock::LockArgs),

    /// Install/expose exactly what rig.lock says
    Sync(commands::sync::SyncArgs),

    /// Validate toolchains + expose freshness
    #[command(visible_aliases = ["doctor", "dr"])]
    Check(commands::check::CheckArgs),

    /// Build host project + ensure expose artifacts are current
    Build(commands::build::BuildArgs),
}
