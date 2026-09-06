use crate::cli::EcosystemArgs;
use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct AddArgs {
    /// Package specs (name, name@version, git+URL, path:…)
    pub packages: Vec<String>,
    #[command(flatten)]
    pub eco: EcosystemArgs,
    /// Force host language override
    #[arg(long)]
    pub host: Option<String>,
    /// Comma-separated cargo features
    #[arg(long)]
    pub features: Option<String>,
    /// Disable default features (cargo)
    #[arg(long)]
    pub no_default_features: bool,
}
