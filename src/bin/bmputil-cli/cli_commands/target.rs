use bmputil::bmp::BmpMatcher;
use clap::Subcommand;
use color_eyre::eyre::Result;
use log::info;

use crate::CliArguments;

#[derive(Subcommand)]
#[command(arg_required_else_help(true))]
pub enum TargetCommmands
{
	/// Print information about the target power control state
	Power,
}

impl TargetCommmands
{
	pub fn subcommand(&self, cli_args: &CliArguments) -> Result<()>
	{
		match self {
			TargetCommmands::Power => power_command(&cli_args),
		}
	}
}

fn power_command(cli_args: &CliArguments) -> Result<()>
{
	// Try and identify all the probes on the system that are allowed by the invocation
	let matcher = BmpMatcher::from_params(cli_args);
	let mut results = matcher.find_matching_probes();

	// Otherwise, turn the result set into a list and go through them displaying them
	let device = results.pop_single("power").map_err(|kind| kind.error())?;
	let remote = device.bmd_serial_interface()?.remote()?;

	let power = remote.get_target_power_state()?;

	info!("Device target power state: {}", power);

	Ok(())
}
