use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct Globals {
    /// Verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Assume yes for prompts
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,

    /// Print actions without writing
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Ask before mutating (inverse of --yes)
    #[arg(long, global = true)]
    pub ask: bool,

    /// Show command duration
    #[arg(long, global = true)]
    pub time_to_action: bool,
}
