use clap::{Command, Arg, ArgMatches};

use dfu_core::sync::DfuSync;

use dfu_libusb::DfuLibusb;
use dfu_libusb::Error as DfuLibusbError;

pub type DfuDevice<ContextT> = DfuSync<DfuLibusb<ContextT>, DfuLibusbError>;


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

    let mut device: DfuDevice<_> = DfuLibusb::open(&context, 0x1d50, 0x6017, 0, 0).unwrap()
        .override_address(0x08002000);

    device.download(firmware_file, file_size).unwrap();
}


fn main()
{
    env_logger::init();

    let parser = Command::new("Blackmagic Probe Firmware Manager")
        .arg_required_else_help(true)
        .subcommand(Command::new("info"))
        .subcommand(Command::new("flash")
            .arg(Arg::new("firmware_binary")
                .takes_value(true)
                .required(true)
            )
        );

    let matches = parser.get_matches();


    let (subcommand, subcommand_matches) = matches.subcommand()
        .expect("No subcommand given!"); // Should be impossible, thanks to clap.

    match subcommand {
        "info" => unimplemented!(),
        "flash" => flash(subcommand_matches),
        &_ => unimplemented!(),
    };
}
