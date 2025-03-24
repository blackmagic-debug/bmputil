// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2023 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
#![cfg_attr(feature = "backtrace", feature(backtrace))]
#[cfg(feature = "backtrace")]
use std::backtrace::BacktraceStatus;

use std::thread;
use std::rc::Rc;
use std::io::Write;
use std::io::Read;
use std::str::FromStr;
use std::time::Duration;

use anstyle;
use clap::{ArgAction, Command, Arg, ArgMatches, crate_version, crate_description, crate_name};
use clap::builder::styling::Styles;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, info, warn, error};

mod usb;
mod error;
mod bmp;
mod elf;
#[cfg(windows)]
mod windows;
mod metadata;

use crate::bmp::{BmpDevice, BmpMatcher, FirmwareType, FirmwareFormat};
use crate::error::{Error, ErrorKind, ErrorSource};
use crate::metadata::download_metadata;

#[macro_export]
#[doc(hidden)]
macro_rules! S
{
    ($expr:expr) => {
        String::from($expr)
    };
}


fn intel_hex_error() -> !
{
    // We're ignoring errors for setting the color because the most important thing
    // is getting the message itself out.
    // If the messages themselves don't write, though, then we might as well just panic.
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);
    let _res = stderr.set_color(ColorSpec::new().set_fg(Some(Color::Red)));
    write!(&mut stderr, "Error: ")
        .expect("failed to write to stderr");
    let _res = stderr.reset();
    writeln!(
        &mut stderr,
        "The specified firmware file appears to be an Intel HEX file, but Intel HEX files are not \
        currently supported. Please use a binary file (e.g. blackmagic.bin), \
        or an ELF (e.g. blackmagic.elf) to flash.",
    )
    .expect("failed to write to stderr");

    std::process::exit(1);
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
    let filename = matches.get_one::<String>("firmware_binary").map(|s| s.as_str())
        .expect("No firmware file was specified!"); // Should be impossible, thanks to clap.
    let firmware_file = std::fs::File::open(filename)
        .map_err(|source| ErrorKind::FirmwareFileIo(Some(filename.to_string())).error_from(source))
        .map_err(|e| e.with_ctx("reading firmware file to flash"))?;

    let mut firmware_file = std::io::BufReader::new(firmware_file);

    let mut firmware_data = Vec::new();
    firmware_file.read_to_end(&mut firmware_data).unwrap();

    // FirmwareFormat::detect_from_firmware() needs at least 4 bytes, and
    // FirmwareType::detect_from_firmware() needs at least 8 bytes,
    // but also if we don't even have 8 bytes there's _no way_ this is valid firmware.
    if firmware_data.len() < 8 {
        return Err(
            ErrorKind::InvalidFirmware(Some(S!("less than 8 bytes long"))).error()
        );
    }

    // Extract the actual firmware data from the file, based on the format we're using.
    let format = FirmwareFormat::detect_from_firmware(&firmware_data);
    let firmware_data = match format {
        FirmwareFormat::Binary => firmware_data,
        FirmwareFormat::Elf => elf::extract_binary(&firmware_data)?,
        FirmwareFormat::IntelHex => intel_hex_error(), // FIXME: implement this.
    };


    // Try to find the Black Magic Probe device based on the filter arguments.
    let matcher = BmpMatcher::from_cli_args(matches);
    let mut results = matcher.find_matching_probes();
    // TODO: flashing to multiple BMPs at once should be supported, but maybe we should require some kind of flag?
    let mut dev: BmpDevice = results.pop_single("flash")?;

    // Grab the platform, which we need for firmware type detection, and the port, which we need
    // to find the probe after rebooting.
    let platform = dev.platform();
    let port = dev.port();

    // Detect what kind of firmware this is, using the platform to determine the link address.
    let firmware_type = FirmwareType::detect_from_firmware(platform, &firmware_data)
        .map_err(|e| e.with_ctx("detecting firmware type"))?;

    debug!("Firmware file was detected as {}", firmware_type);

    // But allow the user to override that type, if they *really* know what they are doing.
    let firmware_type = if let Some(location) = matches
        .get_one::<String>("override-firmware-type").map(|s| s.as_str()) {
        if let Some("really") = matches.get_one::<String>("allow-dangerous-options").map(|s| s.as_str()) {
            warn!("Overriding firmware-type detection and flashing to user-specified location ({}) instead!", location);
        } else {
            // We're ignoring errors for setting the color because the most important thing is
            // getting the message itself out.
            // If the messages themselves don't write, though, then we might as well just panic.
            let mut stderr = StandardStream::stderr(ColorChoice::Auto);
            let _res = stderr.set_color(ColorSpec::new().set_fg(Some(Color::Red)));
            write!(&mut stderr, "WARNING: ").expect("failed to write to stderr");
            let _res = stderr.reset();
            writeln!(
                &mut stderr,
                "--override-firmware-type is used to override the firmware type detection and flash \
                a firmware binary to a location other than the one that it seems to be designed for.\n\
                This is a potentially destructive operation and can result in an unbootable device! \
                (can require a second, external JTAG debugger and manual wiring to fix!)\n\
                \nDo not use this option unless you are a firmware developer and really know what you are doing!\n\
                \nIf you are sure this is really what you want to do, run again with --allow-dangerous-options=really"
            ).expect("failed to write to stderr");
            std::process::exit(1);
        };
        if location == "bootloader" {
            FirmwareType::Bootloader
        } else if location == "application" {
            FirmwareType::Application
        } else {
            unreachable!("Clap ensures invalid option cannot be passed to --override-firmware-type");
        }
    } else {
        firmware_type
    };

    let file_size = firmware_data.len();
    let file_size = u32::try_from(file_size)
        .expect("firmware filesize exceeded 32 bits! Firmware binary must be invalid");

    // If we can't get the string descriptors, try to go ahead with flashing anyway.
    // It's unlikely that other control requests will succeed, but the OS might be messing with
    // the string descriptor stuff.
    let _ = writeln!(std::io::stdout(), "Found: {}", dev)
        .map_err(|e| {
            error!("Failed to read string data from Black Magic Probe: {}\nTrying to continue anyway...", e);
        });

    // We need an Rc<T> as [`dfu_core::sync::DfuSync`] requires `progress` to be 'static,
    // so it must be moved into the closure. However, since we need to call .finish() here,
    // it must be owned by both. Hence: Rc<T>.
    // Default template: `{wide_bar} {pos}/{len}`.
    let progress_bar = ProgressBar::new(file_size as u64)
        .with_style(ProgressStyle::default_bar()
            .template(" {percent:>3}% |{bar:50}| {bytes}/{total_bytes} [{binary_bytes_per_sec} {elapsed}]").unwrap()
        );
    let progress_bar = Rc::new(progress_bar);
    let enclosed = Rc::clone(&progress_bar);

    match dev.download(&*firmware_data, file_size, firmware_type, move |flash_pos_delta| {
        // Don't actually print flashing until the erasing has finished.
        if enclosed.position() == 0 {
            if firmware_type == FirmwareType::Application {
                enclosed.println("Flashing...");
            } else {
                enclosed.println("Flashing bootloader...");
            }
        }
        enclosed.inc(flash_pos_delta as u64);
    }) {
        Ok(()) => {
            progress_bar.finish();
            Ok(())
        },
        Err(e) => {
            progress_bar.finish();
            if progress_bar.position() == (file_size as u64) {
                warn!("Possibly spurious error from OS at the very end of flashing: {}", e);
                Ok(())
            } else {
                Err(e)
            }
        },
    }?;

    drop(dev); // Force libusb to free the device.
    thread::sleep(Duration::from_millis(250));

    let dev = bmp::wait_for_probe_reboot(&port, Duration::from_secs(5), "flash")
        .map_err(|e| {
            error!("Black Magic Probe did not re-enumerate after flashing! Invalid firmware?");
            e
        })?;


    let desc = dev.device().device_descriptor().unwrap();

    let product_string = dev
        .handle()
        .read_product_string_ascii(
            &desc
        )
        .map_err(|e| {
            error!("Error reading firmware version after flash! Invalid firmware?");
            e
        })?;

    let version_string = product_string
        .chars()
        .skip("Black Magic Probe ".len())
        .collect::<String>();

    println!("Black Magic Probe successfully rebooted into firmware version {}", version_string);

    Ok(())
}

fn display_releases() -> Result<(), Error>
{
    let metadata = download_metadata()?;
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
            .display_order(2)
            .about("Display information about available downloadable firmware releases")
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

    let res = match subcommand {
        "info" => info_command(subcommand_matches),
        "flash" => flash(subcommand_matches),
        "debug" => match subcommand_matches.subcommand().unwrap() {
            ("detach", detach_matches) => detach_command(detach_matches),
            other => unreachable!("Unhandled subcommand {:?}", other),
        },
        "releases" => display_releases(),
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
