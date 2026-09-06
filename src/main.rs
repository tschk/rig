use anyhow::Result;
use clap::Parser;
use rig::cli::{Cli, Command};
use rig::ops;
use std::process::ExitCode;
use std::time::Instant;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    let start = Instant::now();
    let code = dispatch(&cli)?;
    if cli.globals.time_to_action {
        eprintln!("time-to-action: {:.3}s", start.elapsed().as_secs_f64());
    }
    Ok(code)
}

fn dispatch(cli: &Cli) -> Result<ExitCode> {
    let g = &cli.globals;
    let code = match &cli.cmd {
        Command::Init(a) => ops::init::run(a, g)?,
        Command::Add(a) => ops::add::run(a, g)?,
        Command::Remove(a) => ops::remove::run(a, g)?,
        Command::Upgrade(a) => ops::upgrade::run(a, g)?,
        Command::List(a) => ops::list::run(a, g)?,
        Command::Search(a) => ops::search::run(a, g)?,
        Command::Info(a) => ops::info::run(a, g)?,
        Command::Outdated(a) => ops::outdated::run(a, g)?,
        Command::Lock(a) => ops::lock::run(a, g)?,
        Command::Sync(a) => ops::sync::run(a, g)?,
        Command::Check(a) => ops::check::run(a, g)?,
        Command::Build(a) => ops::build::run(a, g)?,
    };
    Ok(ExitCode::from(code))
}
