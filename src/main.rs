// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
// SPDX-FileContributor: Modified by Rachel Mant <git@dragonmux.network>
#![cfg_attr(feature = "backtrace", feature(backtrace))]
#[cfg(feature = "backtrace")]
use std::backtrace::BacktraceStatus;

use std::str::FromStr;

use anstyle;
use clap::{ArgAction, Command, Arg, ArgMatches, crate_version, crate_description, crate_name};
use clap::builder::styling::Styles;
use directories::ProjectDirs;
use log::{info, error};

mod bmp;
mod error;
mod elf;
mod flasher;
mod metadata;
mod switcher;
mod usb;
#[cfg(windows)]
mod windows;

use crate::bmp::{BmpDevice, BmpMatcher};
use crate::error::{Error, ErrorSource};
use crate::metadata::download_metadata;

#[macro_export]
#[doc(hidden)]
macro_rules! S
{
    ($expr:expr) => {
        String::from($expr)
    };
}

fn detach_command(matches: &ArgMatches) -> Result<(), Error>
{
    let matcher = BmpMatcher::from_cli_args(matches);
    let mut results = matcher.find_matching_probes();
    let dev = results.pop_single("detach")?;

    use crate::usb::DfuOperatingMode::*;
    match dev.operating_mode() {
        Runtime => println!("Requesting device detach from runtime mode to DFU mode..."),
        FirmwareUpgrade => println!("Requesting device detach from DFU mode to runtime mode..."),
    };

    dev.detach_and_destroy()
        .map_err(|e| e.with_ctx("detaching device"))?;

    Ok(())
}

fn flash(matches: &ArgMatches) -> Result<(), Error>
{
    let file_name = matches.get_one::<String>("firmware_binary").map(|s| s.as_str())
        .expect("No firmware file was specified!"); // Should be impossible, thanks to clap.

    // Try to find the Black Magic Probe device based on the filter arguments.
    let matcher = BmpMatcher::from_cli_args(matches);
    let mut results = matcher.find_matching_probes();
    // TODO: flashing to multiple BMPs at once should be supported, but maybe we should require some kind of flag?
    let dev: BmpDevice = results.pop_single("flash")?;

    flasher::flash_probe(matches, dev, file_name.into())
}

fn display_releases(paths: &ProjectDirs) -> Result<(), Error>
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

fn info_command(matches: &ArgMatches) -> Result<(), Error>
{
    let matcher = BmpMatcher::from_cli_args(matches);

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

fn main()
{
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Warn)
        .parse_default_env()
        .init();

    let mut parser = Command::new(crate_name!());
    if cfg!(windows) {
        parser = parser
            .arg(Arg::new("windows-wdi-install-mode")
                .long("windows-wdi-install-mode")
                .required(false)
                .value_parser(u32::from_str)
                .action(ArgAction::Set)
                .global(true)
                .hide(true)
                .help("Internal argument used when re-executing this command to acquire admin for installing drivers")
            );
    }
    parser = parser
        .about(crate_description!())
        .version(crate_version!())
        .styles(style())
        .disable_colored_help(false)
        .arg_required_else_help(true)
        .arg(Arg::new("serial_number")
            .short('s')
            .long("serial")
            .alias("serial-number")
            .required(false)
            .action(ArgAction::Set)
            .global(true)
            .help("Use the device with the given serial number")
        )
        .arg(Arg::new("index")
            .long("index")
            .required(false)
            .value_parser(usize::from_str)
            .action(ArgAction::Set)
            .global(true)
            .help("Use the nth found device (may be unstable!)")
        )
        .arg(Arg::new("port")
            .short('p')
            .long("port")
            .required(false)
            .action(ArgAction::Set)
            .global(true)
            .help("Use the device on the given USB port")
        )
        .arg(Arg::new("allow-dangerous-options")
            .long("allow-dangerous-options")
            .global(true)
            .action(ArgAction::Set)
            .value_parser(["really"])
            .hide(true)
            .help("Allow usage of advanced, dangerous options that can result in unbootable devices (use with heavy caution!)")
        )
        .subcommand(Command::new("info")
            .display_order(0)
            .about("Print information about connected Black Magic Probe devices")
        )
        .subcommand(Command::new("flash")
            .display_order(1)
            .about("Flash new firmware onto a Black Magic Probe device")
            .arg(Arg::new("firmware_binary")
                .action(ArgAction::Set)
                .required(true)
            )
            .arg(Arg::new("override-firmware-type")
                .long("override-firmware-type")
                .required(false)
                .action(ArgAction::Set)
                .value_parser(["bootloader", "application"])
                .hide_short_help(true)
                .help("flash the specified firmware space regardless of autodetected firmware type")
            )
            .arg(Arg::new("force-override-flash")
                .long("force-override-flash")
                .required(false)
                .action(ArgAction::Set)
                .value_parser(["really"])
                .hide(true)
                .help("forcibly override firmware-type autodetection and flash anyway (may result in an unbootable device!)")
            )
        )
        .subcommand(Command::new("releases")
            .display_order(3)
            .about("Display information about available downloadable firmware releases")
        )
        .subcommand(Command::new("switch")
            .display_order(2)
            .about("Switch the firmware being used on a given probe")
            .arg(Arg::new("override-firmware-type")
                .long("override-firmware-type")
                .required(false)
                .action(ArgAction::Set)
                .value_parser(["bootloader", "application"])
                .hide_short_help(true)
                .help("flash the specified firmware space regardless of autodetected firmware type")
            )
            .arg(Arg::new("force-override-flash")
                .long("force-override-flash")
                .required(false)
                .action(ArgAction::Set)
                .value_parser(["really"])
                .hide(true)
                .help("forcibly override firmware-type autodetection and flash anyway (may result in an unbootable device!)")
            )
        );

    let mut debug_subcmd = Command::new("debug")
        .display_order(10)
        .about("Advanced utility commands for developers")
        .arg_required_else_help(true)
        .subcommand_required(true)
        .subcommand(Command::new("detach")
            .about("Request device to switch from runtime mode to DFU mode or vice versa")
        );

    if cfg!(windows) {
        debug_subcmd = debug_subcmd
            // TODO: add a way to uninstall drivers from bmputil as well.
            .subcommand(Command::new("install-drivers")
                .about("Install USB drivers for BMP devices, and quit")
                .arg(Arg::new("force")
                    .long("--force")
                    .required(false)
                    .action(ArgAction::Set)
                    .help("install the driver even if one is already installed")
                )
            );
    }

    parser = parser.subcommand(debug_subcmd);

    let matches = parser.get_matches();

    let (subcommand, subcommand_matches) = matches.subcommand()
        .expect("No subcommand given!"); // Should be impossible, thanks to clap.

    // Minor HACK: these Windows specific subcommands and operations need to be checked and handled
    // before the others.
    #[cfg(windows)]
    {
        // If the install-driver subcommand was explicitly specified, then perform that operation
        // and exit.
        match subcommand {
            "debug" => match subcommand_matches.subcommand() {
                Some(("install-drivers", install_driver_matches)) => {

                    let wdi_install_parent_pid: Option<&u32> = matches
                        .get_one::<u32>("windows-wdi-install-mode");

                    let force: bool = install_driver_matches.contains_id("force");

                    windows::ensure_access(
                        wdi_install_parent_pid.copied(),
                        true, // explicitly_requested.
                        force,
                    );
                    std::process::exit(0);
                },
                _ => (),
            },
            _ => (),
        }

        // Otherwise, potentially install drivers, but still do whatever else the user wanted.
        windows::ensure_access(
            matches
                .get_one::<u32>("windows-wdi-install-mode")
                .copied(),
            false, // explicitly_requested
            false, // force
        );
    }

    // Try to get the application paths available
    let paths = match ProjectDirs::from("org", "black-magic", "bmputil") {
        Some(paths) => paths,
        None => {
            error!("Failed to get program working paths");
            std::process::exit(2);
        }
    };

    let res = match subcommand {
        "info" => info_command(subcommand_matches),
        "flash" => flash(subcommand_matches),
        "debug" => match subcommand_matches.subcommand().unwrap() {
            ("detach", detach_matches) => detach_command(detach_matches),
            other => unreachable!("Unhandled subcommand {:?}", other),
        },
        "releases" => display_releases(&paths),
        "switch" => switcher::switch_firmware(subcommand_matches, &paths),
        &_ => unimplemented!(),
    };

    // Unfortunately, we have to do the printing ourselves, as we need to print a note
    // in the event that backtraces are supported but not enabled.
    if let Err(e) = res {
        println!("Error: {}", e);
        #[cfg(feature = "backtrace")]
        {
            if e.backtrace.status() == BacktraceStatus::Disabled {
                println!("note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace.");
            }
        }

        if cfg!(not(feature = "backtrace")) {
            println!("note: recompile with nightly toolchain and run with `RUST_BACKTRACE=1` environment variable to display a backtrace.");
        }

        std::process::exit(1);
    }
}
