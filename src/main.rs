use std::time::Duration;

use clap::{Command, Arg, ArgMatches};

use dfu_core::sync::DfuSync;
use dfu_libusb::DfuLibusb;
use dfu_libusb::Error as DfuLibusbError;

use rusb::{UsbContext, Direction, RequestType, Recipient};

use anyhow::Result as AResult;

use log::warn;


mod usb;
mod error;
use crate::usb::{Vid, Pid, InterfaceClass, InterfaceSubClass, GenericDescriptorRef, DfuFunctionalDescriptor};
use crate::usb::DfuRequest;
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


fn interface_descriptor_is_dfu(interface_descriptor: &rusb::InterfaceDescriptor) -> bool
{
    interface_descriptor.class_code() == InterfaceClass::APPLICATION_SPECIFIC.0 &&
        interface_descriptor.sub_class_code() == InterfaceSubClass::DFU.0
}


fn detach_device(device: rusb::Device<rusb::Context>) -> Result<(), BmputilError>
{

    let configuration = match device.active_config_descriptor() {
        Ok(desc) => desc,
        Err(rusb::Error::NotFound) => {

            // In the unlikely event that the OS reports the device as unconfigured
            // (possibly because it was only just connected and is still enumerating?),
            // try instead to simply get the first configuration.

            warn!("OS reports Blackmagic Probe device is unconfigured!");
            warn!("Attempting to continue anyway, in case device is still in the process of enumerating.");

            // USB configuration descriptors are 1-indexed, as 0 is considered
            // to be "unconfigured".
            match device.config_descriptor(1) {
                Ok(d) => d,
                Err(e) => {
                    log_and_return!(BmputilError::DeviceSeemsInvalidError {
                        source: Some(e.into()),
                        invalid_thing: String::from("no configuration descriptor exists"),
                    });
                },
            }
        },
        Err(e) => {
            log_and_return!(BmputilError::from(e));
        },
    };

    // Get the descriptor for the DFU interface on the Blackmagic Probe.
    let dfu_interface_descriptor = configuration
        .interfaces()
        .map(|interface| {
            interface
            .descriptors()
            .next()
            .unwrap() // Unwrap fine as we've already established there is at least one interface.
        })
        .find(interface_descriptor_is_dfu)
        .ok_or_else(|| BmputilError::DeviceSeemsInvalidError {
            source: None,
            invalid_thing: String::from("no DFU interfaces"),
        });
    let dfu_interface_descriptor = match dfu_interface_descriptor {
        Ok(d) => d,
        Err(e) => { log_and_return!(e); },
    };

    // Get the data for all the "extra" descriptors that follow the interface descriptor.
    let extra_descriptors: Vec<_> = GenericDescriptorRef::multiple_from_bytes(dfu_interface_descriptor.extra());

    // Iterate through all the "extra" descriptors to find the DFU functional descriptor.
    let dfu_func_desc_bytes: &[u8; DfuFunctionalDescriptor::LENGTH as usize] = extra_descriptors
        .into_iter()
        .find(|descriptor| descriptor.descriptor_type() == DfuFunctionalDescriptor::TYPE)
        .expect("DFU interface does not have a DFU functional descriptor! This shouldn't be possible!")
        .raw[0..DfuFunctionalDescriptor::LENGTH as usize]
        .try_into() // Convert &[u8] to &[u8; LENGTH].
        .unwrap(); // Unwrap fine as we already set the length two lines above.

    let dfu_func_desc = DfuFunctionalDescriptor::copy_from_bytes(dfu_func_desc_bytes)
        .map_err(|desc_convert_err| BmputilError::DeviceSeemsInvalidError {
            source: Some(desc_convert_err.into()),
            invalid_thing: String::from("DFU functional descriptor"),
        });
    let dfu_func_desc = match dfu_func_desc {
        Ok(d) => d,
        Err(e) => { log_and_return!(e); },
    };


    let handle = match device.open() {
        Ok(handle) => handle,
        Err(e @ rusb::Error::Access) => {
            log_and_return!(BmputilError::PermissionsError {
                source: e,
                operation: String::from("open device"),
                context: String::from("detach device"),
            });
        },
        Err(e @ rusb::Error::NoDevice) => {
            log_and_return!(BmputilError::DeviceDisconnectDuringOperationError {
                source: e,
                operation: String::from("open device"),
                context: String::from("detach device"),
            });
        },
        Err(e) => {
            log_and_return!(BmputilError::from(e));
        },
    };


    let request_type = rusb::request_type(
        Direction::Out,
        RequestType::Class,
        Recipient::Interface,
    );
    let timeout_ms = dfu_func_desc.wDetachTimeOut;
    let interface_index = dfu_interface_descriptor.interface_number() as u16;

    let _response = handle.write_control(
        request_type, // bmRequestType
        DfuRequest::Detach as u8, // bRequest
        timeout_ms, // wValue
        interface_index, // wIndex
        &[], // buffer
        Duration::from_secs(5), // timeout for libusb
    )
    .expect("DFU_DETACH request to Blackmagic Probe failed!");

    Ok(())
}


fn detach_if_needed(context: &rusb::Context)
{
    let devices = context.devices()
        .expect("Unable to list USB devices!");

    let mut bmp_application_mode_devices: Vec<rusb::Device<_>> = devices
        .iter()
        .filter(|device| device_matches_vid_pid(device, Vid(0x1d50), Pid(0x6018)))
        .collect();

    let mut bmp_dfu_mode_devices: Vec<rusb::Device<_>> = devices
        .iter()
        .filter(|device| device_matches_vid_pid(device, Vid(0x1d50), Pid(0x6017)))
        .collect();

    if (bmp_application_mode_devices.len() + bmp_dfu_mode_devices.len()) > 1 {
        unimplemented!("Selecting between multiple Blackmagic Probe devices isn't implemented yet, sorry!");
    }

    if let Some(_dfu_mode_dev) = bmp_dfu_mode_devices.pop() {
        // If it's already in DFU mode, there's nothing to do.
        return;
    }

    if let Some(app_mode_dev) = bmp_application_mode_devices.pop() {

        println!("Device is in runtime mode. Requesting switch to DFU mode...");

        detach_device(app_mode_dev)
            .expect("Device failed to detach!");

        println!("Device detached. Waiting a moment for device to re-enumerate...");
        std::thread::sleep(std::time::Duration::from_secs(2)); // FIXME: this should be more dynamic.

        return;
    }

    panic!("No Blackmagic Probe device was found!"); // FIXME: error handling would be nice >.>
}


fn detach(_matches: &ArgMatches)
{
    let context = rusb::Context::new().unwrap();

    let devices = context.devices()
        .expect("Unable to list USB devices!");

    let mut bmp_application_mode_devices: Vec<rusb::Device<_>> = devices
        .iter()
        .filter(|device| device_matches_vid_pid(device, Vid(0x1d50), Pid(0x6018)))
        .collect();

    if bmp_application_mode_devices.len() > 1 {
        unimplemented!("Selecting between multiple Blackmagic Probe devices isn't implemented yet, sorry!");
    }

    if let Some(app_mode_dev) = bmp_application_mode_devices.pop() {

        println!("Detaching device...");
        detach_device(app_mode_dev).expect("Device failed to detach!");
        println!("Device detached.");
        return;
    }

    panic!("No Blackmagic Probe device was found!");
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

    detach_if_needed(&context);

    let mut device: DfuDevice = DfuLibusb::open(&context, 0x1d50, 0x6017, 0, 0).unwrap()
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
            ("detach", detach_matches) => detach(detach_matches),
            ("reattach", _reattach_matches) => unimplemented!(),
            _ => unreachable!(),
        },


        &_ => unimplemented!(),
    };

    Ok(())
}
