use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct RemoveArgs {
    pub packages: Vec<String>,
}
