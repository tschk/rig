use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct ListArgs {
    /// Optional filter substring
    pub query: Option<String>,
}
