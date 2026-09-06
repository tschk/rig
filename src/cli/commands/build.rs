use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct BuildArgs {
    /// Extra args forwarded to host build tool
    #[arg(last = true)]
    pub args: Vec<String>,
}
