use std::str::FromStr;
use std::rc::Rc;

use clap::{Command, Arg, ArgMatches};

use indicatif::{ProgressBar, ProgressStyle};

use anyhow::Result as AResult;
use log::error;


mod usb;
mod error;
mod bmp;
use crate::usb::DfuOperatingMode;
use crate::bmp::{BlackmagicProbeDevice, BlackmagicProbeMatcher, find_matching_probes};
use crate::error::BmputilError;


fn detach_command(matches: &ArgMatches) -> Result<(), BmputilError>
{
    let matcher = BlackmagicProbeMatcher::from_clap_matches(matches);
    let mut results = find_matching_probes(&matcher);
    let dev = results.pop_single("detach")?;

    use crate::usb::DfuOperatingMode::*;
    match dev.operating_mode() {
        Runtime => println!("Requesting device detach from runtime mode to DFU mode..."),
        FirmwareUpgrade => println!("Requesting device detach from DFU mode to runtime mode..."),
    };

    match dev.detach_and_destroy() {
        Ok(()) => (),
        Err(e) => {
            error!("Device failed to detach!");
            return Err(e);
        }
    };

    Ok(())
}


fn flash(matches: &ArgMatches) -> Result<(), BmputilError>
{
    let filename = matches.value_of("firmware_binary")
        .expect("No firmware file was specified!"); // Should be impossible, thanks to clap.
    let firmware_file = std::fs::File::open(filename)
        .map_err(|source| BmputilError::FirmwareFileIOError { source, filename: filename.to_string() })?;

    let file_size = firmware_file.metadata()
        .map_err(|source| BmputilError::FirmwareFileIOError { source, filename: filename.to_string() })?
        .len();
    let file_size = u32::try_from(file_size)
        .expect("firmware filesize exceeded 32 bits! Firmware binary must be invalid");


    let matcher = BlackmagicProbeMatcher::from_clap_matches(matches);
    let mut results = find_matching_probes(&matcher);
    let mut dev: BlackmagicProbeDevice = results.pop_single("flash")?;

    println!("Found: {}", dev);

    if dev.operating_mode() == DfuOperatingMode::Runtime {
        println!("Detaching and entering DFU mode...");
        dev.detach_and_enumerate()?;
    }

    // We need an Rc<T> as [`dfu_core::sync::DfuSync`] requires `progress` to be 'static,
    // so it must be moved into the closure. However, since we need to call .finish() here,
    // it must be owned by both. Hence: Rc<T>.
    // Default template: `{wide_bar} {pos}/{len}`.
    println!("Flashing...");
    let progress_bar = ProgressBar::new(file_size as u64)
        .with_style(ProgressStyle::default_bar()
            .template(" {percent}% |{bar:50}| {bytes}/{total_bytes} [{binary_bytes_per_sec} {elapsed}]")
        );
    let progress_bar = Rc::new(progress_bar);
    let enclosed = Rc::clone(&progress_bar);

    dev.download(firmware_file, file_size, move |delta| {
        enclosed.inc(delta as u64);
    })?;

    progress_bar.finish();

    Ok(())
}

fn info_command(matches: &ArgMatches) -> Result<(), BmputilError>
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


fn main() -> AResult<()>
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

    match subcommand {
        "info" => info_command(subcommand_matches)?,
        "flash" => flash(subcommand_matches)?,
        "debug" => match subcommand_matches.subcommand().unwrap() {
            ("detach", detach_matches) => detach_command(detach_matches)?,
            _ => unreachable!(),
        },


        &_ => unimplemented!(),
    };

    Ok(())
}
