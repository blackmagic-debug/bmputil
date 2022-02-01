use std::io;
use std::io::Seek;
use std::str::FromStr;

use clap::{App, AppSettings, Arg, ArgMatches};

//use usbapi::UsbEnumerate;
//use usbapi::UsbCore;
//use dfu::core::Dfu;

use dfu_libusb::DfuLibusb;
use dfu_libusb::Error as DfuLibusbError;
pub type DfuDevice<C> = dfu_core::sync::DfuSync<DfuLibusb<C>, DfuLibusbError>;


//#[cfg(no)]
fn flash(matches: &ArgMatches)
{
    let firmware_file = matches.value_of("firmware_binary").unwrap();
    let mut firmware_file = std::fs::File::open(firmware_file).unwrap();
    let file_size = u32::try_from(firmware_file.seek(io::SeekFrom::End(0)).unwrap()).unwrap();
    firmware_file.seek(io::SeekFrom::Start(0)).unwrap();

    let context = rusb::Context::new().unwrap();

    //let mut dev_handle = rusb::open_device_with_vid_pid(0x1d50, 0x6017).unwrap();
    //let timeout = std::time::Duration::from_secs(3);
    //let languages = dev_handle.read_languages(timeout).unwrap();
    //let language = languages
        //.iter()
        //.next()
        //.unwrap();

    //// Claim the DFU interface.
    //dev_handle.claim_interface(0).unwrap();
    //// Get the configuration descriptor.
    //let config_descriptor = dev_handle.device().config_descriptor(0).unwrap();
    //let iface = config_descriptor
        //.interfaces()
        //.find(|iface| iface.number() == 0)
        //.expect("Interface with number not found");
    //let iface_descriptor = iface
        //.descriptors()
        //.find(|desc| desc.setting_number() == 0)
        //.expect("Interface with alt setting not found");

    //let iface_string = dev_handle.read_interface_string(*language, &iface_descriptor, timeout).unwrap();
    //dbg!(&iface_string);
    //for s in iface_string.split(',') {
        //dbg!(s);
    //}

    let layout_str = "08*64Kg";
    let layout = dfu_core::memory_layout::MemoryLayout::try_from(layout_str).unwrap();

    let mut device: DfuDevice<rusb::Context> = DfuLibusb::open(&context, 0x1d50, 0x6017, 0, 0).unwrap();
    //let functional_descriptor = device.functional_descriptor();
    device.download(firmware_file, file_size);

    //let enumerate = UsbEnumerate::from_sysfs()
        //.expect("Cannot enumerate USB devices!");
    //let devices = enumerate.devices();

    //// FIXME: Allow specification of serial number to find the right device.

    //// UsbEnumerate::devices() returns a HashMap where the key is a string of the bus number and
    //// the device number, like `3-5` for bus 3 device 5. Why that's a string when all the other API
    //// functions that consume these values take integers is beyond me, but that means we need to
    //// parse them.

    //let (busnum_devnum, usbfs_dev) = devices.iter()
        //.find(|(_busnum_devnum, device)| device.device.id_vendor == 0x1d50 && device.device.id_product == 0x6017)
        //.expect("Did not find a Blackmagic Probe device");

    //let mut busnum_devnum = busnum_devnum.split('-');
    //let busnum: u8 = busnum_devnum.next()
        //.expect("The USBFS string provided by the library was invalid (shouldn't be possible)")
        //.parse()
        //.unwrap();

    //let devnum: u8 = busnum_devnum.next()
        //.expect("The USBFS string provided by the library was invalid (shouldn't be possible)")
        //.parse()
        //.unwrap();

    //let mut usbdev = UsbCore::from_bus_device(busnum, devnum).unwrap();

    //let descriptors = usbdev.descriptors().as_ref().unwrap();
    //let configuration = &descriptors.device.configurations.get(0).unwrap();

    //dbg!(&configuration);

    //let blackmagic_dfu = Dfu::from_bus_device(busnum, devnum, 4, 0)
        //.expect(&format!("No DFU device was found at {}-{}", busnum, devnum));

    ////dbg!(&blackmagic_dfu);
}


#[cfg(no)]
fn flash(matches: &ArgMatches)
{
    let enumerate = UsbEnumerate::from_sysfs()
        .expect("Cannot enumerate USB devices!");
    let devices = enumerate.devices();

    let (busnum_devnum, usbfs_dev) = devices.iter()
        .find(|(_busnum_devnum, device)| device.device.id_product == 0x6017)
        .expect("Did not find a Blackmagic Probe device!");

    let mut busnum_devnum = busnum_devnum.split('-');
    let busnum: u8 = busnum_devnum.next()
        .expect("The USBFS string was invalid (shouldn't be possible)")
        .parse()
        .unwrap();

    let devnum: u8 = busnum_devnum.next()
        .expect("The USBFS string was invalid (shouldn't be possible)")
        .parse()
        .unwrap();


    let mut usbdev = UsbCore::from_bus_device(busnum, devnum).unwrap();
    usbdev.claim_interface(0).unwrap();
    usbdev.set_interface(0, 0).unwrap();
    let memlayout_string = usbdev.get_descriptor_string_iface(0x0409, 4).unwrap();
    let memlayout = dfu::MemoryLayout::from_str(&memlayout_string).unwrap();
    dbg!(&memlayout);

    //let mut blackmagic_dfu = Dfu::from_bus_device(busnum, devnum, 0, 0)
        //.expect("No DFU device was found!");
}


fn main()
{
    env_logger::init();
    let app = App::new("Blackmagic Probe Firmware Manager")
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .subcommand(App::new("info"))
        .subcommand(App::new("flash")
            .arg(Arg::new("firmware_binary")
                .takes_value(true)
                .required(true)
            )
        );

    let matches = app.get_matches();


    let (subcommand, subcommand_matches) = matches.subcommand()
        .expect("No subcommand given!"); // Should be impossible.

    match subcommand {
        "info" => unimplemented!(),
        "flash" => flash(subcommand_matches),
        &_ => unimplemented!(),
    };
}
