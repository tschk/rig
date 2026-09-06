use crate::cli::EcosystemArgs;
use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct UpgradeArgs {
    /// Packages to upgrade (all if omitted)
    pub packages: Vec<String>,
    #[command(flatten)]
    pub eco: EcosystemArgs,
}
