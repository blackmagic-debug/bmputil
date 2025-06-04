// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
// SPDX-FileContributor: Modified by Rachel Mant <git@dragonmux.network>

use std::ffi::OsStr;
use std::str::FromStr;

use clap::builder::TypedValueParser;
use clap::{Arg, ArgAction, Args, Command, Parser, Subcommand};
use clap::builder::styling::Styles;
use color_eyre::eyre::{Context, Result};
use directories::ProjectDirs;
use log::{info, error};

use bmputil::{AllowDangerous, BmpParams, FlashParams};
use bmputil::bmp::{BmpDevice, BmpMatcher, FirmwareType};
use bmputil::metadata::download_metadata;
#[cfg(windows)]
use bmputil::windows;

#[derive(Parser)]
#[command(version, about, styles(style()), disable_colored_help(false), arg_required_else_help(true))]
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

#[derive(Subcommand)]
enum ToplevelCommmands
{
    /// Actions to be performed against a probe
    Probe(ProbeArguments),
}

#[derive(Args)]
struct ProbeArguments
{
    #[arg(global = true, long = "allow-dangerous-options", hide = true, default_value_t = AllowDangerous::Never)]
    #[arg(value_enum)]
    /// Allow usage of advanced, dangerous options that can result in unbootable devices (use with heavy caution!)
    allow_dangerous_options: AllowDangerous,

    #[command(subcommand)]
    subcommand: ProbeCommmands,
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
    // Reboot a Black Magic Probe (potentially into its bootloader)
    Reboot(RebootArguments),
    #[cfg(windows)]
    /// Install USB drivers for BMP devices, and quit
    InstallDrivers(DriversArguments),
}

#[derive(Args)]
struct InfoArguments
{
    #[arg(long = "list-targets", default_value_t = false)]
    list_targets: bool,
}

#[derive(Args)]
struct UpdateArguments
{
    firmware_binary: String,
    #[arg(long = "override-firmware-type", hide_short_help = true, value_enum)]
    /// Flash the specified firmware space regardless of autodetected firmware type
    override_firmware_type: Option<FirmwareType>,
    #[arg(long = "force-override-flash", hide = true, default_value_t = false, value_parser = ConfirmedBoolParser {})]
    #[arg(action = ArgAction::Set)]
    /// Forcibly override firmware type autodetection and Flash anyway (may result in an unbootable device!)
    force_override_flash: bool,

    #[command(subcommand)]
    subcommand: Option<UpdateCommands>
}

#[derive(Subcommand)]
enum UpdateCommands
{
    /// List available releases and firmware that can be downloaded
    List
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

impl FlashParams for CliArguments
{
    fn allow_dangerous_options(&self) -> AllowDangerous
    {
        match &self.subcommand {
            ToplevelCommmands::Probe(probe_args) => probe_args.allow_dangerous_options,
            _ => AllowDangerous::Never,
        }
    }

    fn override_firmware_type(&self) -> Option<FirmwareType>
    {
        match &self.subcommand {
            ToplevelCommmands::Probe(probe_args) => match &probe_args.subcommand {
                ProbeCommmands::Update(flash_args) => flash_args.override_firmware_type,
                ProbeCommmands::Switch(switch_args) => switch_args.override_firmware_type,
                _ => None,
            }
            _ => None,
        }
    }
}

#[derive(Clone)]
struct ConfirmedBoolParser {}

impl TypedValueParser for ConfirmedBoolParser
{
    type Value = bool;

    fn parse_ref(
        &self, cmd: &Command, _arg: Option<&Arg>, value: &OsStr,
    ) -> Result<Self::Value, clap::Error>
    {
        let value = value.to_str().ok_or_else(|| {
            clap::Error::new(clap::error::ErrorKind::InvalidUtf8).with_cmd(cmd)
        })?;
        Ok(value == "really")
    }
}

fn reboot_command(cli_args: &CliArguments, reboot_args: &RebootArguments) -> Result<()>
{
    let matcher = BmpMatcher::from_params(cli_args);
    let mut results = matcher.find_matching_probes();
    let mut dev = results
        .pop_single("detach")
        .map_err(|kind| kind.error())?;

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
        }
        FirmwareUpgrade => println!("Rebooting probe into firmware...")
    }

    dev.detach_and_destroy().wrap_err("detaching device")
}

fn update_probe(cli_args: &CliArguments, flash_args: &UpdateArguments) -> Result<()>
{
    let file_name = flash_args.firmware_binary.as_str();

    // Try to find the Black Magic Probe device based on the filter arguments.
    let matcher = BmpMatcher::from_params(cli_args);
    let mut results = matcher.find_matching_probes();
    // TODO: flashing to multiple BMPs at once should be supported, but maybe we should require some kind of flag?
    let dev: BmpDevice = results
        .pop_single("flash")
        .map_err(|kind| kind.error())?;

    bmputil::flasher::flash_probe(cli_args, dev, file_name.into())
}

fn display_releases(paths: &ProjectDirs) -> Result<()>
{
    // Figure out where the metadata cache is
    let cache = paths.cache_dir();
    // Acquire the metadata for display
    let metadata = download_metadata(cache)?;
    // Loop through all the entries and display them
    for (version, release) in metadata.releases {
        info!("Details of release {}:", version);
        info!("-> Release includes BMDA builds? {}", release.includes_bmda);
        info!("-> Release done for probes: {}", release.firmware.keys().map(|p| p.to_string()).collect::<Vec<_>>().join(", "));
        for (probe, firmware) in release.firmware {
            info!("-> probe {} has {} firmware variants", probe.to_string(), firmware.variants.len());
            for (variant, download) in firmware.variants {
                info!("  -> Firmware variant {}", variant);
                info!("    -> {} will be downloaded as {}", download.friendly_name, download.file_name.display());
                info!("    -> Variant will be downloaded from {}", download.uri);
            }
        }
        if let Some(bmda) = release.bmda {
            info!("-> Release contains BMDA for {} OSes", bmda.len());
            for (os, bmda_arch) in bmda {
                info!("  -> {} release is for {} architectures", os.to_string(), bmda_arch.binaries.len());
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

fn info_command(cli_args: &CliArguments) -> Result<()>
{
    let matcher = BmpMatcher::from_params(cli_args);

    let mut results = matcher.find_matching_probes();

    let devices = results.pop_all()?;

    let multiple = devices.len() > 1;
    for (index, dev) in devices.iter().enumerate() {

        println!("Found: {}", dev);

        // If we have multiple connected probes, then additionally display their index
        // and print a trailing newline.
        if multiple {
            println!("  Index:  {}\n", index);
        }
    }

    Ok(())
}

/// Clap v3 style (approximate)
/// See https://stackoverflow.com/a/75343828
fn style() -> clap::builder::Styles {
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
        .literal(
            anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Green))),
        )
}

fn main() -> Result<()>
{
    color_eyre::install()?;
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Warn)
        .parse_default_env()
        .init();

    let cli_args = CliArguments::parse();

    // If the user hasn't requested us to go installing drivers explicitly, make sure that we
    // actually have sufficient permissions here to do what is needed
    #[cfg(windows)]
    match cli_args.subcommand {
        ToplevelCommmands::Probe(ProbeArguments { subcommand: ProbeCommmands::InstallDrivers(_), .. }) => (),
        // Potentially install drivers, but still do whatever else the user wanted.
        _ => {
            windows::ensure_access(
                cli_args.windows_wdi_install_mode,
                false, // explicitly_requested
                false, // force
            );
        }
    }

    // Try to get the application paths available
    let paths = match ProjectDirs::from("org", "black-magic", "bmputil") {
        Some(paths) => paths,
        None => {
            error!("Failed to get program working paths");
            std::process::exit(2);
        }
    };

    match &cli_args.subcommand {
        ToplevelCommmands::Probe(probe_args) => match &probe_args.subcommand {
            ProbeCommmands::Info(_) => info_command(&cli_args),
            ProbeCommmands::Update(update_args) => {
                if let Some(subcommand) = &update_args.subcommand {
                    match subcommand {
                        UpdateCommands::List => display_releases(&paths),
                    }
                } else {
                    update_probe(&cli_args, update_args)
                }
            }
            ProbeCommmands::Switch(_) => bmputil::switcher::switch_firmware(&cli_args, &paths),
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
