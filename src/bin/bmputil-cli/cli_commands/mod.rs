// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by P-Storm <pauldeman@gmail.com>
// SPDX-FileContributor: Modified by P-Storm <pauldeman@gmail.com>

use bmputil::bmp::FirmwareType;
use bmputil::{AllowDangerous, BmpParams, FlashParams};
use clap::Subcommand;
use directories::ProjectDirs;
use log::error;

use crate::cli_commands::probe::ProbeArguments;
use crate::{CliArguments, CompletionArguments};

pub mod probe;

#[derive(Subcommand)]
pub enum ToplevelCommmands
{
	/// Actions to be performed against a probe
	Probe(ProbeArguments),
	/// Actions to be performed against a target connected to a probe
	Target,
	/// Actions that run the tool as a debug/tracing server
	Server,
	/// Actions that run debugging commands against a target connected to a probe
	Debug,
	/// Generate completions data for the shell
	Complete(CompletionArguments),
}

impl FlashParams for CliArguments
{
	fn allow_dangerous_options(&self) -> AllowDangerous
	{
		match &self.subcommand {
			ToplevelCommmands::Probe(probe_args) => probe_args.allow_dangerous_options(),
			_ => AllowDangerous::Never,
		}
	}

	fn override_firmware_type(&self) -> Option<FirmwareType>
	{
		match &self.subcommand {
			ToplevelCommmands::Probe(probe_args) => probe_args.override_firmware_type(),
			_ => None,
		}
	}
}

impl BmpParams for CliArguments
{
	fn index(&self) -> Option<usize>
	{
		self.index
	}

	fn serial_number(&self) -> Option<&str>
	{
		self.serial_number.as_deref()
	}
}

fn paths() -> ProjectDirs
{
	// Try to get the application paths available
	match ProjectDirs::from("org", "black-magic", "bmputil") {
		Some(paths) => paths,
		None => {
			error!("Failed to get program working paths");
			std::process::exit(2);
		},
	}
}
