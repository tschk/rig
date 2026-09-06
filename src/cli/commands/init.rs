use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct InitArgs {
    /// Overwrite existing rig.toml
    #[arg(long, short = 'f')]
    pub force: bool,
    /// Force host language (rust|zig|nim|c|cpp|v|d|odin|hare|csharp)
    #[arg(long)]
    pub host: Option<String>,
}
