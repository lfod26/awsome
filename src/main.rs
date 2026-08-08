mod aws_command;
mod cli;
mod client;
mod config;
mod interactive;
mod logger;
mod schedule;
mod signal;
mod spinner;
mod ssh_setup;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command, ConfigureAction};
use client::{Ec2Instance, start_ssh_proxy};
use config::{AwsomeConfig, ProfileGroup};
use ssh_setup::setup_ssh;

fn main() -> Result<()> {
    signal::install_ctrlc_handler()?;

    let cli = Cli::parse();

    let is_command_default = cli.command.is_none();
    let command = cli.command.unwrap_or(Command::Start {
        schedule_shutdown: None,
    });

    let mut config = AwsomeConfig::load()?;

    if let Command::Configure { action } = command {
        return match action {
            ConfigureAction::Show => {
                println!("{config}");
                Ok(())
            }

            ConfigureAction::Select { index } => config.set_selected(index),

            ConfigureAction::Add {
                profile,
                instance_id,
            } => config.add(profile, instance_id),

            ConfigureAction::Remove { index } => config.remove(index),
        };
    }

    let ProfileGroup {
        profile,
        instance_id,
    } = config.get_selected()?;

    match command {
        Command::Configure { .. } => unreachable!("already handled"),

        Command::Start { schedule_shutdown } => {
            let ec2 = Ec2Instance::new_logged_in(profile, instance_id)?;

            ec2.start_and_wait(!is_command_default)?;

            if let Some(time_str) = schedule_shutdown {
                let (minutes, target_time) = schedule::minutes_until_next(&time_str)?;
                ec2.schedule_shutdown(minutes, &target_time.format("%H:%M").to_string())?;
            }

            Ok(())
        }

        Command::Stop => Ec2Instance::new_logged_in(profile, instance_id)?.stop_and_wait(true),

        Command::SetupSsh => {
            let ec2 = Ec2Instance::new_logged_in(profile, instance_id)?;
            setup_ssh(|script| ec2.push_public_key(script))
        }

        Command::SshProxy { port } => start_ssh_proxy(profile, instance_id, port),
    }
}
