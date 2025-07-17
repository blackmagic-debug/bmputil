// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
// SPDX-FileContributor: Modified by P-Storm <pauldeman@gmail.com>

mod cli_commands;

use std::ffi::OsStr;
use std::io::stdout;
use std::str::FromStr;

use bmputil::bmp::{BmpDevice, BmpMatcher, FirmwareType};
use bmputil::metadata::download_metadata;
#[cfg(windows)]
use bmputil::windows;
use bmputil::{AllowDangerous, BmpParams, FlashParams};
use clap::builder::TypedValueParser;
use clap::builder::styling::Styles;
use clap::{Arg, ArgAction, Args, Command, CommandFactory, Parser, Subcommand, crate_description, crate_version};
use clap_complete::{Shell, generate};
use color_eyre::config::HookBuilder;
use color_eyre::eyre::{Context, EyreHandler, InstallError, OptionExt, Result};
use directories::ProjectDirs;
use log::{debug, error, info, warn};
use owo_colors::OwoColorize;

use crate::cli_commands::ToplevelCommmands;
use crate::cli_commands::probe::ProbeArguments;

#[derive(Parser)]
#[command(
	version,
	about = format!("{} v{}", crate_description!(), crate_version!()),
	styles(style()),
	disable_colored_help(false),
	arg_required_else_help(true)
)]
struct CliArguments
{
	#[arg(global = true, short = 's', long = "serial", alias = "serial-number")]
	/// Use the device with the given serial number
	serial_number: Option<String>,
	#[arg(global = true, long = "index", value_parser = usize::from_str)]
	/// Use the nth found device (may be unstable!)
	index: Option<usize>,
	#[arg(global = true, short = 'p', long = "port")]
	/// Use the device on the given USB port
	port: Option<String>,

	#[cfg(windows)]
	#[arg(global = true, long = "windows-wdi-install-mode", value_parser = u32::from_str, hide = true)]
	/// Internal argument used when re-executing this command to acquire admin for installing drivers
	windows_wdi_install_mode: Option<u32>,

	#[command(subcommand)]
	pub subcommand: ToplevelCommmands,
}

#[derive(Args)]
struct CompletionArguments
{
	shell: Shell,
}

#[derive(Clone)]
struct ConfirmedBoolParser {}

impl TypedValueParser for ConfirmedBoolParser
{
	type Value = bool;

	fn parse_ref(&self, cmd: &Command, _arg: Option<&Arg>, value: &OsStr) -> Result<Self::Value, clap::Error>
	{
		let value = value
			.to_str()
			.ok_or_else(|| clap::Error::new(clap::error::ErrorKind::InvalidUtf8).with_cmd(cmd))?;
		Ok(value == "really")
	}
}

type EyreHookFunc = Box<dyn Fn(&(dyn std::error::Error + 'static)) -> Box<dyn EyreHandler> + Send + Sync + 'static>;
type PanicHookFunc = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync + 'static>;

struct BmputilHook
{
	inner_hook: EyreHookFunc,
}

struct BmputilPanic
{
	inner_hook: PanicHookFunc,
}

struct BmputilHandler
{
	inner_handler: Box<dyn EyreHandler>,
}

impl BmputilHook
{
	fn build_handler(&self, error: &(dyn std::error::Error + 'static)) -> BmputilHandler
	{
		BmputilHandler {
			inner_handler: (*self.inner_hook)(error),
		}
	}

	pub fn install(self) -> Result<(), InstallError>
	{
		color_eyre::eyre::set_hook(self.into_eyre_hook())
	}

	pub fn into_eyre_hook(self) -> EyreHookFunc
	{
		Box::new(move |err| Box::new(self.build_handler(err)))
	}
}

impl BmputilPanic
{
	pub fn install(self)
	{
		std::panic::set_hook(self.into_panic_hook());
	}

	pub fn into_panic_hook(self) -> PanicHookFunc
	{
		Box::new(move |panic_info| {
			self.print_header();
			(*self.inner_hook)(panic_info);
			self.print_footer();
		})
	}

	fn print_header(&self)
	{
		eprintln!("------------[ ✂ cut here ✂ ]------------");
		eprintln!("Unhandled crash in bmputil-cli v{}", crate_version!());
		eprintln!();
	}

	fn print_footer(&self)
	{
		eprintln!();
		eprintln!("{}", "Please include all lines down to this one from the cut here".yellow());
		eprintln!("{}", "marker, and report this issue to our issue tracker at".yellow());
		eprintln!("https://github.com/blackmagic-debug/bmputil/issues");
	}
}

impl EyreHandler for BmputilHandler
{
	fn debug(&self, error: &(dyn std::error::Error + 'static), fmt: &mut core::fmt::Formatter<'_>)
	-> core::fmt::Result
	{
		writeln!(fmt, "------------[ ✂ cut here ✂ ]------------")?;
		write!(fmt, "Unhandled crash in bmputil-cli v{}", crate_version!())?;
		self.inner_handler.debug(error, fmt)?;
		writeln!(fmt)?;
		writeln!(fmt)?;
		writeln!(
			fmt,
			"{}",
			"Please include all lines down to this one from the cut here".yellow()
		)?;
		writeln!(fmt, "{}", " marker, and report this issue to our issue tracker at".yellow())?;
		write!(fmt, "https://github.com/blackmagic-debug/bmputil/issues")
	}

	fn track_caller(&mut self, location: &'static std::panic::Location<'static>)
	{
		self.inner_handler.track_caller(location);
	}
}

fn install_error_handler() -> Result<()>
{
	// Grab us a new default handler
	let default_handler = HookBuilder::default();
	// Turn that into a pair of hooks - one for panic, and the other for errors
	let (panic_hook, eyre_hook) = default_handler.try_into_hooks()?;

	// Make an instance of our custom handler, paassing it the panic one to do normal panic
	// handling with, so we only have to deal with our additions, and install it
	BmputilPanic {
		inner_hook: panic_hook.into_panic_hook(),
	}
	.install();

	// Make an instance of our custom handler, passing it the default one to do the main
	// error handling with, so we only have to deal with our additions, and install it
	BmputilHook {
		inner_hook: eyre_hook.into_eyre_hook(),
	}
	.install()?;
	Ok(())
}

/// Clap v3 style (approximate)
/// See https://stackoverflow.com/a/75343828
fn style() -> clap::builder::Styles
{
	Styles::styled()
		.usage(
			anstyle::Style::new()
				.fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Yellow)))
				.bold(),
		)
		.header(
			anstyle::Style::new()
				.bold()
				.fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Yellow))),
		)
		.literal(anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Green))))
}

fn main() -> Result<()>
{
	install_error_handler()?;
	env_logger::Builder::new()
		.filter_level(log::LevelFilter::Info)
		.parse_default_env()
		.init();

	let cli_args = CliArguments::parse();

	// If the user hasn't requested us to go installing drivers explicitly, make sure that we
	// actually have sufficient permissions here to do what is needed
	#[cfg(windows)]
	match cli_args.subcommand {
		ToplevelCommmands::Probe(ProbeArguments {
			subcommand: ProbeCommmands::InstallDrivers(_),
			..
		}) => (),
		// Potentially install drivers, but still do whatever else the user wanted.
		_ => {
			windows::ensure_access(
				cli_args.windows_wdi_install_mode,
				false, // explicitly_requested
				false, // force
			);
		},
	}

	match &cli_args.subcommand {
		ToplevelCommmands::Probe(probe_args) => probe_args.subcommand(&cli_args),
		ToplevelCommmands::Target => {
			warn!("Command space reserved for future tool version");
			Ok(())
		},
		ToplevelCommmands::Server => {
			warn!("Command space reserved for future tool version");
			Ok(())
		},
		ToplevelCommmands::Debug => {
			warn!("Command space reserved for future tool version");
			Ok(())
		},
		ToplevelCommmands::Complete(comp_args) => {
			let mut cmd = CliArguments::command();
			generate(comp_args.shell, &mut cmd, "bmputil-cli", &mut stdout());
			Ok(())
		},
	}
}
