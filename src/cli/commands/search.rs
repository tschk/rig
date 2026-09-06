use crate::cli::EcosystemArgs;
use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct SearchArgs {
    pub query: String,
    #[command(flatten)]
    pub eco: EcosystemArgs,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}
