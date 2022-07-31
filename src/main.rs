#![cfg_attr(feature = "backtrace", feature(backtrace))]
#[cfg(feature = "backtrace")]
use std::backtrace::BacktraceStatus;

use std::thread;
use std::rc::Rc;
use std::io::Write;
use std::str::FromStr;
use std::time::Duration;

use clap::{Command, Arg, ArgMatches};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, warn, error};


mod usb;
mod error;
mod bmp;
use crate::bmp::{BlackmagicProbeDevice, BlackmagicProbeMatcher, BmpFirmwareType, find_matching_probes};
use crate::error::{Error, ErrorKind, ErrorSource};

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
    let matcher = BlackmagicProbeMatcher::from_clap_matches(matches);
    let mut results = find_matching_probes(&matcher);
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
    let filename = matches.value_of("firmware_binary")
        .expect("No firmware file was specified!"); // Should be impossible, thanks to clap.
    let firmware_file = std::fs::File::open(filename)
        .map_err(|source| ErrorKind::FirmwareFileIo(Some(filename.to_string())).error_from(source))
        .map_err(|e| e.with_ctx("reading firmware file to flash"))?;


    // Detect what kind of firmware this is.
    let firmware_type = BmpFirmwareType::detect_from_firmware(&firmware_file);
    let firmware_type = if let Err(e @ Error { kind: ErrorKind::FirmwareIsElf | ErrorKind::FirmwareIsHex, ..}) = firmware_type {

        let kind = match e.kind {
            ErrorKind::FirmwareIsElf => "ELF",
            ErrorKind::FirmwareIsHex => "Intel HEX",
            _ => unreachable!("ErrorKind can only be FirmwareIsElf or FirmwareIsHex"),
        };

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
            "The specified firmware file appears to be an {kind} file, but {kind} files are not \
            currently supported. Please use a binary file (e.g. blackmagic.bin) to flash",
            kind = kind,
        )
        .expect("failed to write to stderr");
        std::process::exit(1);
    } else {
        firmware_type?
    };

    debug!("Firmware file was detected as {}", firmware_type);

    // But allow the user to override that type, if they *really* know what they are doing.
    let firmware_type = if let Some(location) = matches.value_of("override-firmware-type") {
        if let Some("really") = matches.value_of("allow-dangerous-options") {
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
            BmpFirmwareType::Bootloader
        } else if location == "application" {
            BmpFirmwareType::Application
        } else {
            unreachable!("Clap ensures invalid option cannot be passed to --override-firmware-type");
        }
    } else {
        firmware_type
    };

    let file_size = firmware_file.metadata()
        .map_err(|source| ErrorKind::FirmwareFileIo(Some(filename.to_string())).error_from(source))?
        .len();

    let file_size = u32::try_from(file_size)
        .expect("firmware filesize exceeded 32 bits! Firmware binary must be invalid");


    let matcher = BlackmagicProbeMatcher::from_clap_matches(matches);
    let mut results = find_matching_probes(&matcher);
    let mut dev: BlackmagicProbeDevice = results.pop_single("flash")?;

    let port = dev.port();

    println!("Found: {}", dev);

    // We need an Rc<T> as [`dfu_core::sync::DfuSync`] requires `progress` to be 'static,
    // so it must be moved into the closure. However, since we need to call .finish() here,
    // it must be owned by both. Hence: Rc<T>.
    // Default template: `{wide_bar} {pos}/{len}`.
    let progress_bar = ProgressBar::new(file_size as u64)
        .with_style(ProgressStyle::default_bar()
            .template(" {percent:>3}% |{bar:50}| {bytes}/{total_bytes} [{binary_bytes_per_sec} {elapsed}]")
        );
    let progress_bar = Rc::new(progress_bar);
    let enclosed = Rc::clone(&progress_bar);

    println!("Erasing flash...");
    match dev.download(&firmware_file, file_size, firmware_type, move |delta| {
        if enclosed.position() == 0 {
            if firmware_type == BmpFirmwareType::Application {
                enclosed.println("Flashing...");
            } else {
                enclosed.println("Flashing bootloader...");
            }
        }
        enclosed.inc(delta as u64);
    }) {
        Ok(v) => Ok(v),
        Err(e) => match e.kind {
            ErrorKind::External(ErrorSource::DfuLibusb(dfu_libusb::Error::Io(source))) => {
                Err(ErrorKind::FirmwareFileIo(Some(filename.to_string())).error_from(Box::new(source)))
            },
            _ => {
                Err(e)
            },
        },
    }?;

    progress_bar.finish();

    drop(dev); // Force libusb to free the device.
    thread::sleep(Duration::from_millis(250));

    let dev = bmp::wait_for_probe_reboot(&port, Duration::from_secs(5), "flash")
        .map_err(|e| {
            error!("Black Magic Probe did not re-enumerate after flashing! Invalid firmware?");
            e
        })?;


    let languages = dev
        .handle()
        .read_languages(Duration::from_secs(2))
        .map_err(|e| {
            error!("Error reading firmware version after flash! Invalid firmware?");
            e
        })?;

    let desc = dev.device().device_descriptor().unwrap();

    let product_string = dev
        .handle()
        .read_product_string(
            *languages.first().unwrap(),
            &desc,
            Duration::from_secs(2),
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

fn info_command(matches: &ArgMatches) -> Result<(), Error>
{
    let matcher = BlackmagicProbeMatcher::from_clap_matches(matches);

    let mut results = find_matching_probes(&matcher);

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


fn main()
{
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Warn)
        .parse_default_env()
        .init();

    let parser = Command::new("Blackmagic Probe Firmware Manager")
        .arg_required_else_help(true)
        .arg(Arg::new("serial_number")
            .short('s')
            .long("serial")
            .alias("serial-number")
            .required(false)
            .takes_value(true)
            .global(true)
            .help("Use the device with the given serial number")
        )
        .arg(Arg::new("index")
            .long("index")
            .required(false)
            .takes_value(true)
            .global(true)
            .validator(|arg| usize::from_str(arg))
            .help("Use the nth found device (may be unstable!)")
        )
        .arg(Arg::new("port")
            .short('p')
            .long("port")
            .required(false)
            .takes_value(true)
            .global(true)
            .help("Use the device on the given USB port")
        )
        .arg(Arg::new("allow-dangerous-options")
            .long("allow-dangerous-options")
            .global(true)
            .takes_value(true)
            .possible_value("really")
            .hide(true)
            .help("Allow usage of advanced, dangerous options that can result in unbootable devices (use with heavy caution!)")
        )
        .subcommand(Command::new("info")
            .display_order(0)
            .about("Print information about connected Blackmagic Probe devices")
        )
        .subcommand(Command::new("flash")
            .display_order(1)
            .about("Flash new firmware onto a Blackmagic Probe device")
            .arg(Arg::new("firmware_binary")
                .takes_value(true)
                .required(true)
            )
            .arg(Arg::new("override-firmware-type")
                .long("override-firmware-type")
                .required(false)
                .takes_value(true)
                .possible_values(&["bootloader", "application"])
                .hide_short_help(true)
                .help("flash the specified firmware space regardless of autodetected firmware type")
            )
            .arg(Arg::new("force-override-flash")
                .long("force-override-flash")
                .required(false)
                .takes_value(true)
                .possible_value("really")
                .hide(true)
                .help("forcibly override firmware-type autodetection and flash anyway (may result in an unbootable device!)")
            )
        )
        .subcommand(Command::new("debug")
            .display_order(10)
            .about("Advanced utility commands for developers")
            .arg_required_else_help(true)
            .subcommand(Command::new("detach")
                .about("Request device to switch from runtime mode to DFU mode or vice versa")
            )
        );

    let matches = parser.get_matches();


    let (subcommand, subcommand_matches) = matches.subcommand()
        .expect("No subcommand given!"); // Should be impossible, thanks to clap.

    let res = match subcommand {
        "info" => info_command(subcommand_matches),
        "flash" => flash(subcommand_matches),
        "debug" => match subcommand_matches.subcommand().unwrap() {
            ("detach", detach_matches) => detach_command(detach_matches),
            _ => unreachable!(),
        },


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
