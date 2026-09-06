use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct CheckArgs {
    /// Attempt to repair stale expose
    #[arg(long)]
    pub fix: bool,
    /// Run fuller toolchain probes
    #[arg(long)]
    pub full: bool,
}
