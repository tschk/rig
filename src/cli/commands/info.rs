use crate::cli::EcosystemArgs;
use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct InfoArgs {
    pub package: String,
    #[command(flatten)]
    pub eco: EcosystemArgs,
}
