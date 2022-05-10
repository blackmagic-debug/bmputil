use clap::{Command, Arg, ArgMatches};

use dfu_core::sync::DfuSync;
use dfu_libusb::DfuLibusb;
use dfu_libusb::Error as DfuLibusbError;

use rusb::UsbContext;

use anyhow::Result as AResult;


mod usb;
mod error;
mod bmp;
use crate::usb::{Vid, Pid, DfuOperatingMode};
use crate::bmp::BlackmagicProbeDevice;
use crate::error::BmputilError;


type DfuDevice = DfuSync<DfuLibusb<rusb::Context>, DfuLibusbError>;


fn device_matches_vid_pid<ContextT>(device: &rusb::Device<ContextT>, vid: Vid, pid: Pid) -> bool
where
    ContextT: UsbContext,
{
    let dev_descriptor = device.device_descriptor()
        .expect(libusb_cannot_fail!("libusb_get_device_descriptor()"));
    let dev_vid = dev_descriptor.vendor_id();
    let dev_pid = dev_descriptor.product_id();

    (dev_vid == vid.0) && (dev_pid == pid.0)
}


fn detach_device(device: rusb::Device<rusb::Context>) -> Result<(), BmputilError>
{

    let device = BlackmagicProbeDevice::from_usb_device(device)?;

    use crate::usb::DfuOperatingMode::*;
    match device.operating_mode() {
        Runtime => println!("Requesting device detach from runtime mode to DFU mode..."),
        FirmwareUpgrade => println!("Requesting device detach from DFU mode to runtime mode..."),
    };

    device.request_detach().expect("Device failed to detach!");

    Ok(())
}


fn detach_command(_matches: &ArgMatches)
{
    let context = rusb::Context::new().unwrap();

    // HACK FIXME: this is cursed.
    let (dev, _handle, _mode) = BlackmagicProbeDevice::first_found()
        .expect("Failed to open Blackmagic Probe device")
        .into_inner_parts();
    detach_device(dev)
        .expect("Failed to detach device");
}


fn flash(matches: &ArgMatches)
{
    let firmware_file = matches.value_of("firmware_binary")
        .expect("No firmware file was specified!"); // Should be impossible, thanks to clap.
    let firmware_file = std::fs::File::open(firmware_file)
        .unwrap_or_else(|e| panic!("{}: Failed to open firmware file {}", e, firmware_file));

    let file_size = firmware_file.metadata()
        .expect("Failed to get length of the firmware binary")
        .len();
    let file_size = u32::try_from(file_size)
        .expect("firmware filesize exceeded 32 bits! Firmware binary must be invalid");

    let context = rusb::Context::new().unwrap();

    let dev = BlackmagicProbeDevice::first_found()
        .expect("Unable to open Blackmagic Probe device");

    // If the device is in runtime mode, then we need to switch it to DFU mode
    // before we can actually do the firmware upgrade.
    if dev.operating_mode() == DfuOperatingMode::Runtime {
        dev.request_detach()
            .expect("Failed to detach device");
    }
    std::thread::sleep(std::time::Duration::from_secs(1));

    let (vid, pid) = (BlackmagicProbeDevice::VID, BlackmagicProbeDevice::PID_DFU);

    let mut device: DfuDevice = DfuLibusb::open(&context, vid.0, pid.0, 0, 0).unwrap()
        .override_address(0x08002000);

    println!("Performing flash...");
    device.download(firmware_file, file_size).unwrap();
}


fn main() -> AResult<()>
{
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Warn)
        .parse_default_env()
        .init();

    let parser = Command::new("Blackmagic Probe Firmware Manager")
        .arg_required_else_help(true)
        .subcommand(Command::new("info"))
        .subcommand(Command::new("flash")
            .arg(Arg::new("firmware_binary")
                .takes_value(true)
                .required(true)
            )
        )
        .subcommand(Command::new("debug")
            .subcommand(Command::new("detach"))
            .subcommand(Command::new("reattach"))
        );

    let matches = parser.get_matches();


    let (subcommand, subcommand_matches) = matches.subcommand()
        .expect("No subcommand given!"); // Should be impossible, thanks to clap.

    match subcommand {
        "info" => unimplemented!(),
        "flash" => flash(subcommand_matches),
        "debug" => match subcommand_matches.subcommand().unwrap() {
            ("detach", detach_matches) => detach_command(detach_matches),
            ("reattach", _reattach_matches) => unimplemented!(),
            _ => unreachable!(),
        },


        &_ => unimplemented!(),
    };

    Ok(())
}
