mod aws_command;
mod cli;
mod client;
mod config;
mod interactive;
mod schedule;
mod signal;
mod spinner;

use anyhow::Result;
use clap::Parser;
use console::style;

use cli::{Cli, Command};
use config::AwsomeConfig;

use crate::client::Ec2Instance;

fn main() -> Result<()> {
    signal::install_ctrlc_handler()?;

    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Command::Start {
        schedule_shutdown: None,
    });

    let mut config = AwsomeConfig::load()?;

    if let Command::Configure { action } = command {
        return match action {
            cli::ConfigureAction::Add {
                profile,
                instance_id,
            } => config.add(profile, instance_id),

            cli::ConfigureAction::Remove { index } => config.remove(index),

            cli::ConfigureAction::Show => Ok(config.show()),

            cli::ConfigureAction::Select { index } => config.set_selected(index),
        };
    }

    if config.is_empty() {
        println!("No configuration found. Run `awsome configure add` first.");
        return Ok(());
    }

    let selected_conf = config.selected_group()?;
    let profile = &selected_conf.profile;
    let instance_id = &selected_conf.instance_id;

    match command {
        Command::Stop => {
            let ec2 = Ec2Instance::new_logged_in(profile, instance_id)?;

            if let Some(state) = ec2.state()?
                && state == "stopped"
            {
                println!(
                    "Instance {instance_id} is already {}.",
                    style("stopped").red()
                );
            } else {
                ec2.stop_and_wait()?;
            }

            Ok(())
        }

        Command::Start { schedule_shutdown } => {
            let ec2 = Ec2Instance::new_logged_in(profile, instance_id)?;

            if let Some(state) = ec2.state()?
                && state == "running"
            {
                println!(
                    "Instance {instance_id} is already {}.",
                    style("running").green()
                );
            } else {
                ec2.start_and_wait()?;
            }

            if let Some(time_str) = schedule_shutdown {
                let (minutes, target_time) = schedule::minutes_until_next(&time_str)?;
                ec2.schedule_shutdown(minutes, &target_time.format("%H:%M").to_string())?;
            }

            Ok(())
        }

        Command::Configure { .. } => {
            unreachable!("already handled above")
        }
    }
}
