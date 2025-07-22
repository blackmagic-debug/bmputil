// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by P-Storm <pauldeman@gmail.com>
// SPDX-FileContributor: Modified by P-Storm <pauldeman@gmail.com>

use bmputil::AllowDangerous;
use bmputil::bmp::{BmpDevice, BmpMatcher, FirmwareType};
use bmputil::metadata::download_metadata;
use clap::{ArgAction, Args, Subcommand};
use color_eyre::eyre::{Context, OptionExt};
use directories::ProjectDirs;
use log::{debug, error, info, warn};

use crate::cli_commands::paths;
use crate::{CliArguments, ConfirmedBoolParser};

#[derive(Args)]
pub struct ProbeArguments
{
	#[arg(global = true, long = "allow-dangerous-options", hide = true, default_value_t = AllowDangerous::Never)]
	#[arg(value_enum)]
	/// Allow usage of advanced, dangerous options that can result in unbootable devices (use with heavy caution!)
	allow_dangerous_options: AllowDangerous,

	#[command(subcommand)]
	subcommand: ProbeCommmands,
}

impl ProbeArguments
{
	pub fn allow_dangerous_options(&self) -> AllowDangerous
	{
		self.allow_dangerous_options
	}

	pub fn override_firmware_type(&self) -> Option<FirmwareType>
	{
		match &self.subcommand {
			ProbeCommmands::Update(flash_args) => flash_args.override_firmware_type,
			ProbeCommmands::Switch(switch_args) => switch_args.override_firmware_type,
			_ => None,
		}
	}

	pub fn subcommand(&self, cli_args: &CliArguments) -> color_eyre::Result<()>
	{
		let paths = paths();
		match &self.subcommand {
			ProbeCommmands::Info(info_args) => info_command(&cli_args, info_args),
			ProbeCommmands::Update(update_args) => {
				if let Some(subcommand) = &update_args.subcommand {
					match subcommand {
						UpdateCommands::List => display_releases(&paths),
					}
				} else {
					update_probe(&cli_args, update_args, &paths)
				}
			},
			ProbeCommmands::Switch(_) => bmputil::switcher::switch_firmware(cli_args, &paths),
			ProbeCommmands::Reboot(reboot_args) => reboot_command(&cli_args, reboot_args),
			#[cfg(windows)]
			ProbeCommmands::InstallDrivers(driver_args) => {
				windows::ensure_access(
					cli_args.windows_wdi_install_mode,
					true, // explicitly_requested.
					driver_args.force,
				);
				Ok(())
			},
		}
	}
}

#[derive(Subcommand)]
#[command(arg_required_else_help(true))]
enum ProbeCommmands
{
	/// Print information about connected Black Magic Probe devices
	Info(InfoArguments),
	/// Update the firmware running on a Black Magic Probe
	Update(UpdateArguments),
	/// Switch the firmware being used on a given probe
	Switch(SwitchArguments),
	/// Reboot a Black Magic Probe (potentially into its bootloader)
	Reboot(RebootArguments),
	#[cfg(windows)]
	/// Install USB drivers for BMP devices, and quit
	InstallDrivers(DriversArguments),
}

#[derive(Args)]
struct InfoArguments
{
	#[arg(long = "list-targets", default_value_t = false)]
	/// List the target supported by a particular probe (if the firmware is new enough)
	list_targets: bool,
}

fn info_command(cli_args: &CliArguments, info_args: &InfoArguments) -> color_eyre::Result<()>
{
	// Try and identify all the probes on the system that are allowed by the invocation
	let matcher = BmpMatcher::from_params(cli_args);
	let mut results = matcher.find_matching_probes();

	// If we were invoked to list the targets supported by a specific probe, dispatch to the function for that
	if info_args.list_targets {
		return list_targets(results.pop_single("list targets").map_err(|kind| kind.error())?);
	}

	// Otherwise, turn the result set into a list and go through them displaying them
	let devices = results.pop_all()?;
	let multiple = devices.len() > 1;

	for (index, dev) in devices.iter().enumerate() {
		debug!("Probe identity: {}", dev.firmware_identity()?);
		println!("Found: {dev}");

		// If we have multiple connected probes, then additionally display their index
		// and print a trailing newline.
		if multiple {
			println!("  Index:  {index}\n");
		}
	}

	Ok(())
}

#[derive(Args)]
struct UpdateArguments
{
	firmware_binary: Option<String>,
	#[arg(long = "override-firmware-type", hide_short_help = true, value_enum)]
	/// Flash the specified firmware space regardless of autodetected firmware type
	override_firmware_type: Option<FirmwareType>,
	#[arg(long = "force-override-flash", hide = true, default_value_t = false, value_parser = ConfirmedBoolParser {})]
	#[arg(action = ArgAction::Set)]
	/// Forcibly override firmware type autodetection and Flash anyway (may result in an unbootable device!)
	force_override_flash: bool,

	#[arg(short = 'f', long = "force", default_value_t = false)]
	/// Force the current latest release onto the probe even if the probe is running ostensibly newer firmware
	force: bool,

	#[arg(long = "use-rc", default_value_t = false)]
	/// Allow the tool to use release candidates as possible upgrade targets when considering the latest release
	use_rc: bool,

	#[command(subcommand)]
	subcommand: Option<UpdateCommands>,
}

#[derive(Subcommand)]
enum UpdateCommands
{
	/// List available releases and firmware that can be downloaded
	List,
}

#[derive(Args)]
struct SwitchArguments
{
	#[arg(long = "override-firmware-type", hide_short_help = true, value_enum)]
	/// Flash the specified firmware space regardless of autodetected firmware type
	override_firmware_type: Option<FirmwareType>,
	#[arg(long = "force-override-flash", hide = true, default_value_t = false, value_parser = ConfirmedBoolParser {})]
	#[arg(action = ArgAction::Set)]
	/// Forcibly override firmware type autodetection and Flash anyway (may result in an unbootable device!)
	force_override_flash: bool,
}

#[derive(Args)]
#[group(multiple = false)]
struct RebootArguments
{
	#[arg(long = "dfu", default_value_t = false)]
	dfu: bool,
	#[arg(long = "repeat", default_value_t = false)]
	repeat: bool,
}

#[cfg(windows)]
#[derive(Args)]
struct DriversArguments
{
	#[arg(long = "force", default_value_t = false)]
	/// Install the driver even if one is already installed
	force: bool,
}

fn display_releases(paths: &ProjectDirs) -> color_eyre::Result<()>
{
	// Figure out where the metadata cache is
	let cache = paths.cache_dir();
	// Acquire the metadata for display
	let metadata = download_metadata(cache)?;
	// Loop through all the entries and display them
	for (version, release) in metadata.releases {
		info!("Details of release {version}:");
		info!("-> Release includes BMDA builds? {}", release.includes_bmda);
		info!(
			"-> Release done for probes: {}",
			release
				.firmware
				.keys()
				.map(|p| p.to_string())
				.collect::<Vec<_>>()
				.join(", ")
		);
		for (probe, firmware) in release.firmware {
			info!(
				"-> probe {} has {} firmware variants",
				probe.to_string(),
				firmware.variants.len()
			);
			for (variant, download) in firmware.variants {
				info!("  -> Firmware variant {}", variant);
				info!(
					"    -> {} will be downloaded as {}",
					download.friendly_name,
					download.file_name.display()
				);
				info!("    -> Variant will be downloaded from {}", download.uri);
			}
		}
		if let Some(bmda) = release.bmda {
			info!("-> Release contains BMDA for {} OSes", bmda.len());
			for (os, bmda_arch) in bmda {
				info!(
					"  -> {} release is for {} architectures",
					os.to_string(),
					bmda_arch.binaries.len()
				);
				for (arch, binary) in bmda_arch.binaries {
					info!("    -> BMDA binary for {}", arch.to_string());
					info!("    -> Name of executable in archive: {}", binary.file_name.display());
					info!("    -> Archive will be downloaded from {}", binary.uri);
				}
			}
		}
	}
	Ok(())
}

fn list_targets(probe: BmpDevice) -> color_eyre::Result<()>
{
	// Extract the remote protocol interface for the probe
	let remote = probe.bmd_serial_interface()?.remote()?;
	// Ask it what architectures it supports, and display that
	let archs = remote.supported_architectures()?;
	if let Some(archs) = archs {
		info!("Probe supports the following target architectures: {archs}");
	} else {
		info!("Could not determine what target architectures your probe supports - please upgrade your firmware.");
	}
	// Ask it what target families it supports, and display that
	let families = remote.supported_families()?;
	if let Some(families) = families {
		info!("Probe supports the following target families: {families}");
	} else {
		info!("Could not determine what target families your probe supports - please upgrade your firmware.");
	}
	Ok(())
}

fn reboot_command(cli_args: &CliArguments, reboot_args: &RebootArguments) -> color_eyre::Result<()>
{
	let matcher = BmpMatcher::from_params(cli_args);
	let mut results = matcher.find_matching_probes();
	let mut dev = results.pop_single("detach").map_err(|kind| kind.error())?;

	use bmputil::usb::DfuOperatingMode::*;

	if reboot_args.dfu {
		return match dev.operating_mode() {
			Runtime => {
				println!("Rebooting probe into bootloader...");
				dev.detach_and_destroy().wrap_err("detaching device")
			},
			FirmwareUpgrade => {
				println!("Probe already in bootloader, nothing to do.");
				Ok(())
			},
		};
	}
	if reboot_args.repeat {
		println!("Switching probe between bootloader and firmware...");
		return dev.detach_and_destroy().wrap_err("detaching device");
	}

	match dev.operating_mode() {
		Runtime => {
			println!("Rebooting probe...");
			// This'll take us from the firmware into the bootloader
			dev.detach_and_enumerate().wrap_err("detaching device")?;
			// Now take us back in the post-match step
		},
		FirmwareUpgrade => println!("Rebooting probe into firmware..."),
	}

	dev.detach_and_destroy().wrap_err("detaching device")
}

fn update_probe(cli_args: &CliArguments, flash_args: &UpdateArguments, paths: &ProjectDirs) -> color_eyre::Result<()>
{
	use bmputil::switcher::{download_firmware, pick_firmware};

	// Try to find the Black Magic Probe device based on the filter arguments.
	let matcher = BmpMatcher::from_params(cli_args);
	let mut results = matcher.find_matching_probes();
	let probe = results.pop_single("flash").map_err(|kind| kind.error())?;

	// Figure out what file should be written to the probe - if there's something on the command line that takes
	// precedence, otherwise we use the metadata to pick the most recent full release
	let file_name = match &flash_args.firmware_binary {
		Some(file_path) => file_path.into(),
		None => {
			// Grab the probe's identity for its version string
			let identity = &probe.firmware_identity()?;
			// Grab the current metadata to be able to figure out what the latest is
			let cache = paths.cache_dir();
			let metadata = download_metadata(cache)?;
			// Extract the most recent release from the metadata
			let (latest_version, latest_release) = metadata
				.latest(flash_args.use_rc)
				.ok_or_eyre("Could not determine the latest release of the firmware")?;
			// Extract the matching firmware for the probe
			let latest_firmware = latest_release.firmware.get(
				&identity
					.variant()
					.ok_or_eyre("Device appears to be in bootloader, so cannot determine probe type")?,
			);
			let latest_firmware = if let Some(firmware) = latest_firmware {
				firmware
			} else {
				// Otherwise, if we didn't find a suitable firmware version, error out
				error!("Cannot find suitable firmware for your probe from the pre-built releases");
				return Ok(());
			};

			// Check whether the release is newer than the firmware on the probe, and if it is, pick that as the file.
			// If it is not, print a message and exit successfully. This check is bypassed when `--force` is given
			// which makes this command push the selected firmware to the probe anyway
			if identity.version >= latest_version && !flash_args.force {
				info!(
					"Latest release {} is not newer than firmware version {}, not updating",
					latest_version, identity.version
				);
				return Ok(());
			}
			// Convert the version number to a string for display and use with the switcher
			let latest_version_str = latest_version.to_string();
			if identity.version < latest_version {
				info!("Upgrading probe firmware from {} to {}", identity.version, latest_version_str);
			} else if flash_args.force {
				warn!(
					"Forcibly downgrading firmware from {} to {}",
					identity.version, latest_version_str
				);
			}

			// If there's more than one variant in this release, defer to the switcher engine to pick the
			// variant that will be used. Otherwise, jump below to the flasher system with the file
			// name for that firmware version, having downloaded it
			let firmware_variant = match latest_firmware.variants.len() {
				// If there's exactly one variant, call the switcher system to download it then use that
				// file as the result here
				1 => latest_firmware.variants.values().next().unwrap(),
				// There's more than one variant? okay, ask the switcher system to have the user tell us
				// which to use then.
				_ => match pick_firmware(latest_version_str.as_str(), latest_firmware)? {
					Some(variant) => variant,
					None => {
						println!("firmware variant selection cancelled, stopping operation");
						return Ok(());
					},
				},
			};

			download_firmware(firmware_variant, paths.cache_dir())?
		},
	};

	bmputil::flasher::flash_probe(cli_args, probe, file_name)
}
