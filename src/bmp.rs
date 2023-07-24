// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2023 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
use std::mem;
use std::thread;
use std::io::Read;
use std::cell::{RefCell, Ref, RefMut};
use std::str::FromStr;
use std::time::{Duration, Instant};
use std::fmt::{self, Display, Formatter};
use std::array::TryFromSliceError;

use clap::ArgMatches;
use log::{trace, debug, info, warn, error};
use rusb::{UsbContext, Direction, RequestType, Recipient};
use dfu_libusb::{DfuLibusb, Error as DfuLibusbError};
use dfu_core::{State as DfuState, Error as DfuCoreError};

use crate::{libusb_cannot_fail, S};
use crate::error::{Error, ErrorKind, ErrorSource, ResErrorKind};
use crate::usb::{DfuFunctionalDescriptor, InterfaceClass, InterfaceSubClass, GenericDescriptorRef, DfuRequest};
use crate::usb::{Vid, Pid, DfuOperatingMode};

type UsbDevice = rusb::Device<rusb::Context>;
type UsbHandle = rusb::DeviceHandle<rusb::Context>;


/// Semantically represents a Black Magic Probe USB device.
#[derive(Debug, PartialEq, Eq)]
pub struct BmpDevice
{
    device: RefCell<Option<UsbDevice>>,
    handle: RefCell<Option<UsbHandle>>,

    /// The operating mode (application or DFU) the BMP is currently in.
    mode: DfuOperatingMode,

    /// The platform this BMP is running on.
    platform: BmpPlatform,

    /// RefCell for interior-mutability-based caching.
    serial: RefCell<Option<String>>,

    /// RefCell for interior-mutability-based caching.
    port: RefCell<Option<String>>,
}

impl BmpDevice
{
    pub fn from_usb_device(device: UsbDevice) -> Result<Self, Error>
    {
        let desc = device.device_descriptor()
            .expect(libusb_cannot_fail!("libusb_get_device_descriptor()"));
        let (vid, pid) = (Vid(desc.vendor_id()), Pid(desc.product_id()));
        let (platform, mode) = BmpPlatform::from_vid_pid(vid, pid).ok_or_else(|| {
            warn!("Device passed to BmpDevice::from_usb_device() does not seem to be a BMP device!");
            warn!("The logic for finding this device is probably incorrect!");
            ErrorKind::DeviceNotFound.error()
        })?;

        let handle = device.open()?;


        Ok(Self {
            device: RefCell::new(Some(device)),
            mode,
            platform,
            handle: RefCell::new(Some(handle)),
            serial: RefCell::new(None),
            port: RefCell::new(None),
        })
    }

    /// Get the [`rusb::Device<rusb::Context>`] associated with the connected Black Magic Probe.
    #[allow(dead_code)]
    pub fn device(&self) -> Ref<UsbDevice>
    {
        let dev = self.device.borrow();
        Ref::map(dev, |d| d.as_ref().expect("Unreachable: self.device is None"))
    }

    /// Violate struct invariants if you want. I'm not the boss of you.
    #[allow(dead_code)]
    pub unsafe fn device_mut(&mut self) -> RefMut<UsbDevice>
    {
        let dev = self.device.borrow_mut();
        RefMut::map(dev, |d| d.as_mut().expect("Unreachable: self.device is None"))
    }

    /// Get the [`rusb::DeviceHandle<rusb::Context>`] associated with the connected Black Magic Probe.
    #[allow(dead_code)]
    pub fn handle(&self) -> Ref<UsbHandle>
    {
        let handle = self.handle.borrow();
        Ref::map(handle, |h| h.as_ref().expect("Unreachable: self.handle is None"))
    }

    /// Violate struct invariants if you want. I'm not the boss of you.
    #[allow(dead_code)]
    pub unsafe fn handle_mut(&mut self) -> RefMut<UsbHandle>
    {
        let handle = self.handle.borrow_mut();
        RefMut::map(handle, |h| h.as_mut().expect("Unreachable: self.handle is None"))
    }

    /// The safe but internal version of [handle_mut].
    fn _handle_mut(&mut self) -> RefMut<UsbHandle>
    {
        unsafe { self.handle_mut() }
    }

    pub fn operating_mode(&self) -> DfuOperatingMode
    {
        self.mode
    }

    pub fn platform(&self) -> BmpPlatform
    {
        self.platform
    }

    /// Returns a the serial number string for this device.
    ///
    /// This struct caches the serial number in an [`std::cell::RefCell`],
    /// and thus returns a `Ref<str>` rather than the `&str` directly.
    /// Feel free to clone the result if you want a directly referenceable value.
    pub fn serial_number(&self) -> Result<Ref<str>, Error>
    {
        let serial = self.serial.borrow();
        if serial.is_some() {
            return Ok(Ref::map(serial, |s| s.as_deref().unwrap()));
        }
        // If we don't have a serial yet, drop this borrow so we can re-borrow
        // self.serial as mutable later.
        drop(serial);

        let languages = self.handle().read_languages(Duration::from_secs(2))?;
        if languages.is_empty() {
            return Err(
                ErrorKind::DeviceSeemsInvalid(String::from("no string descriptor languages"))
                    .error()
            );
        }

        let language = languages.first().unwrap(); // Okay as we proved len > 0.

        let serial = self
            .handle()
            .read_serial_number_string(
                *language,
                &self.device().device_descriptor().unwrap(),
                Duration::from_secs(2),
            )?;

        // Finally, now that we have the serial number, cache it...
        *self.serial.borrow_mut() = Some(serial);

        // And return it.
        Ok(Ref::map(self.serial.borrow(), |s| s.as_deref().unwrap()))
    }


    /// Returns a string that represents the full port of the device, in the format of
    /// `<bus>-<port>.<subport>.<subport...>`.
    ///
    /// This is theoretically reliable, but is also OS-reported, so it doesn't *have* to be, alas.
    pub fn port(&self) -> String
    {
        if let Some(port) = self.port.borrow().as_ref() {
            return port.to_string();
        }

        let bus = self.device().bus_number();
        let path = self
            .device()
            .port_numbers()
            .expect("unreachable: rusb always provides a properly sized array to libusb_get_port_numbers()")
            .into_iter()
            .map(|v| v.to_string())
            .collect::<Vec<String>>()
            .as_slice()
            .join(".");

        let port = format!("{}-{}", bus, path);
        let ret = port.clone();
        self.port.replace(Some(port));

        ret
    }

    /// Return a string suitable for display to the user.
    ///
    /// Note: this performs USB IO to retrieve the necessary string descriptors, if those strings
    /// have not yet been retrieved previously (and thus not yet cached).
    pub fn display(&self) -> Result<String, Error>
    {
        let handle = self.handle();
        let mut languages = handle
            .read_languages(Duration::from_secs(2))
            .map_err(|e| Error::from(e).with_ctx("reading supported string descriptor langauges"))?;

        let first_lang = languages.pop()
            .ok_or_else(|| ErrorKind::DeviceSeemsInvalid(S!("no supported string descriptor languages")).error())?;

        let dev_desc = &self
            .device()
            .device_descriptor()
            .expect(libusb_cannot_fail!("libusb_get_device_descriptor()"));

        let product_string = handle
            .read_product_string(
                first_lang,
                dev_desc,
                Duration::from_secs(2),
            )
            .map_err(|e| ErrorKind::DeviceSeemsInvalid(S!("no product string descriptor")).error_from(e))?;

        let serial = self.serial_number()?;

        Ok(format!("{}\n  Serial: {}\n  Port:  {}", product_string, serial, self.port()))
    }

    /// Find and return the DFU functional descriptor and its interface number for the connected Black Magic Probe device.
    ///
    /// Unfortunately this only returns the DFU interface's *number* and not the interface or
    /// descriptor itself, as there are ownership issues with that and rusb does not yet
    /// implement the proper traits (like. Clone.) for its types for this to work properly.
    ///
    /// This does not execute any requests to the device, and only uses information already
    /// available from libusb's device structures.
    pub fn dfu_descriptors(&self) -> Result<(u8, DfuFunctionalDescriptor), Error>
    {
        let configuration = match self.device().active_config_descriptor() {
            Ok(d) => d,
            Err(rusb::Error::NotFound) => {
                // In the unlikely even that the OS reports the device as unconfigured
                // (possibly because it was only just connected and is still enumerating?)
                // try instead to simply get the first configuration, and hope that the
                // device is configured by the time we try to send requests to it.
                // I'm not actually sure this case is even possibly on any OS, but might
                // as well check.

                warn!("OS reports Black Magic Probe device is unconfigured!");
                warn!("Attempting to continue anyway, in case the device is still in the process of enumerating.");

                // USB configurations are 1-indexed, as 0 is considered
                // to be "unconfigured".
                match self.device().config_descriptor(1) {
                    Ok(d) => d,
                    Err(e) => {
                        return Err(
                            ErrorKind::DeviceSeemsInvalid(
                                String::from("no configuration descriptor exists")
                            ).error_from(e)
                        );
                    },
                }
            },
            Err(e) => {
                return Err(e.into());
            },
        };

        let dfu_interface_descriptor = configuration
            .interfaces()
            .map(|interface| {
                interface
                .descriptors()
                .next()
                .unwrap() // Unwrap fine as we've already established that there is at least one interface.
            })
            .find(|desc| {
                desc.class_code() == InterfaceClass::APPLICATION_SPECIFIC.0 &&
                    desc.sub_class_code() == InterfaceSubClass::DFU.0

            })
            .ok_or_else(|| ErrorKind::DeviceSeemsInvalid(String::from("no DFU interfaces")).error())?;

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
            .map_err(|source| {
                ErrorKind::DeviceSeemsInvalid(String::from("DFU functional descriptor"))
                    .error_from(source)
            })?;

        Ok((dfu_interface_descriptor.interface_number(), dfu_func_desc))
    }

    /// Requests the device to leave DFU mode, using the DefuSe extensions.
    fn leave_dfu_mode(&mut self) -> Result<(), Error>
    {
        debug!("Attempting to leave DFU mode...");
        let (iface_number, _func_desc) = self.dfu_descriptors()?;
        self._handle_mut().claim_interface(iface_number)?;

        let request_type = rusb::request_type(
            Direction::Out,
            RequestType::Class,
            Recipient::Interface,
        );

        // Perform the zero-length DFU_DNLOAD request.
        let _response = self.handle().write_control(
            request_type, // bmRequestType
            DfuRequest::Dnload as u8, // bRequest
            0, // wValue
            0, // wIndex
            &[], // data
            Duration::from_secs(2),
        )?;

        // Then perform a DFU_GETSTATUS request to complete the leave "request".
        let request_type = rusb::request_type(
            Direction::In,
            RequestType::Class,
            Recipient::Interface,
        );

        let mut buf: [u8; 6] = [0; 6];
        let status = self.handle().read_control(
            request_type, // bmRequestType
            DfuRequest::GetStatus as u8, // bRequest
            0, // wValue
            iface_number as u16, // wIndex
            &mut buf,
            Duration::from_secs(2),
        )?;

        trace!("Device status after zero-length DNLOAD is 0x{:02x}", status);
        info!("DFU_GETSTATUS request completed. Device should now re-enumerate into runtime mode.");

        match self._handle_mut().release_interface(iface_number) {
            // Ignore if the device has already disconnected.
            Err(rusb::Error::NoDevice) => Ok(()),
            other => other,
        }?;


        Ok(())
    }

    /// Performs a DFU_DETACH request to enter DFU mode.
    fn enter_dfu_mode(&mut self) -> Result<(), Error>
    {
        let (iface_number, func_desc) = self.dfu_descriptors()?;
        self._handle_mut().claim_interface(iface_number)?;

        let request_type = rusb::request_type(
            Direction::Out,
            RequestType::Class,
            Recipient::Interface,
        );
        let timeout_ms = func_desc.wDetachTimeOut;

        let _response = self.handle().write_control(
            request_type, // bmpRequestType
            DfuRequest::Detach as u8, // bRequest
            timeout_ms, // wValue
            iface_number as u16, // wIndex
            &[], // buffer
            Duration::from_secs(1), // timeout for libusb
        )
        .map_err(Error::from)
        .map_err(|e| e.with_ctx("sending control request"))?;

        info!("DFU_DETACH request completed. Device should now re-enumerate into DFU mode.");

        match self._handle_mut().release_interface(iface_number) {
            // Ignore if the device has already disconnected.
            Err(rusb::Error::NoDevice) => Ok(()),
            other => other,
        }?;

        Ok(())
    }

    /// Requests the Black Magic Probe device to detach, switching from DFU mode to runtime mode or vice versa. You probably want [`detach_and_enumerate`].
    ///
    /// This function does not re-enumerate the device and re-initialize this structure, and thus after
    /// calling this function, the this [`BmpDevice`] instance will not be in a correct state
    /// if the device successfully detached. Further requests will fail, and functions like
    /// `dfu_descriptors()` may return now-incorrect data.
    pub unsafe fn request_detach(&mut self) -> Result<(), Error>
    {
        use DfuOperatingMode::*;
        let res = match self.mode {
            Runtime => self.enter_dfu_mode(),
            FirmwareUpgrade => self.leave_dfu_mode(),
        };
        match res {
            Ok(()) => (),
            Err(e) => return Err(e),
        };

        Ok(())
    }

    /// Requests the Black Magic Probe to detach, and re-initializes this struct with the new
    /// device.
    pub fn detach_and_enumerate(&mut self) -> Result<(), Error>
    {
        // Save the port for finding the device again after.
        let port = self.port();

        if cfg!(not(windows)) {
            unsafe { self.request_detach()? };
        } else {
            // HACK: WinUSB seems to have a race condition where it can spuriously give ERROR_GEN_FAILURE
            // (which becomes LIBUSB_ERROR_PIPE) when a control request results in a device disconnect.
            use crate::ErrorSource::Libusb;
            let res = unsafe { self.request_detach() };
            if let Err(e @ Error { kind: ErrorKind::External(Libusb(rusb::Error::Pipe)), .. }) = res {
                warn!("Possibly spurious error from Windows when attempting to detach: {}", e);
            } else {
                res?;
            }
        }

        // Now drop the device so libusb doesn't re-grab the same thing.
        drop(self.device.take());
        drop(self.handle.take());

        // TODO: make this sleep() timeout configurable?
        thread::sleep(Duration::from_millis(500));

        // Now try to find the device again on that same port.
        let dev = wait_for_probe_reboot(&port, Duration::from_secs(5), "flash")?;

        // If we've made it here, then we have successfully re-found the device.
        // Re-initialize this structure from the new data.
        *self = dev;

        Ok(())
    }

    /// Detach the Black Magic Probe device, consuming the structure.
    ///
    /// Currently there is not a way to recover this instance if this function errors.
    /// You'll just have to create another one.
    pub fn detach_and_destroy(mut self) -> Result<(), Error>
    {
        if cfg!(not(windows)) {
            unsafe { self.request_detach()? };
        } else {
            // HACK: WinUSB seems to have a race condition where it can spuriously give ERROR_GEN_FAILURE
            // (which becomes LIBUSB_ERROR_PIPE) when a control request results in a device disconnect.
            use crate::ErrorSource::Libusb;
            let res = unsafe { self.request_detach() };
            if let Err(e @ Error { kind: ErrorKind::External(Libusb(rusb::Error::Pipe)), .. }) = res {
                warn!("Possibly spurious error from Windows when attempting to detach: {}", e);
            } else {
                res?;
            }
        }

        Ok(())
    }

    /// Downloads firmware onto the device, switching into DFU mode automatically if necessary.
    ///
    /// `progress` is a callback of the form `fn(just_written: usize)`, for callers to keep track of
    /// the flashing process.
    pub fn download<'r, R, P>(&mut self, firmware: &'r R, length: u32, firmware_type: FirmwareType, progress: P) -> Result<(), Error>
    where
        &'r R: Read,
        R: ?Sized,
        P: Fn(usize) + 'static,
    {
        if self.mode == DfuOperatingMode::Runtime {
            self.detach_and_enumerate()
                .map_err(|e| e.with_ctx("detaching device for download"))?;
        }

        let load_address = self.platform.load_address(firmware_type);

        let mut dfu_dev = DfuLibusb::from_usb_device(
            self.device().clone(),
            self.handle.take().expect("Must have a valid device handle"),
            0,
            0,
        )?;
        dfu_dev
            .with_progress(progress)
            .override_address(load_address);

        debug!("Load address: 0x{:08x}", load_address);
        info!("Performing flash...");

        let res = dfu_dev.download(firmware, length)
            .map_err(|source| {
                match source {
                    dfu_libusb::Error::LibUsb(rusb::Error::NoDevice) => {
                        error!("Black Magic Probe device disconnected during the flash process!");
                        warn!(
                            "If the device now fails to enumerate, try holding down the button while plugging the device in order to enter the bootloader."
                        );
                        ErrorKind::DeviceDisconnectDuringOperation.error_from(source)
                    }
                    _ => source.into(),
                }
            });

        if let Err(ErrorKind::External(ErrorSource::DfuLibusb(DfuLibusbError::Dfu(DfuCoreError::StateError(DfuState::DfuError))))) = res.err_kind() {

            warn!("Device reported an error when trying to flash; going to clear status and try one more time...");

            thread::sleep(Duration::from_millis(250));

            let request_type = rusb::request_type(
                Direction::Out,
                RequestType::Class,
                Recipient::Interface,
            );

            self.handle().write_control(
                request_type,
                DfuRequest::ClrStatus as u8,
                0,
                0, // iface number
                &[],
                Duration::from_secs(2),
            )?;

            dfu_dev.download(firmware, length)
                .map_err(|source| {
                    match source {
                        dfu_libusb::Error::LibUsb(rusb::Error::NoDevice) => {
                            error!("Black Magic Probe device disconnected during the flash process!");
                            warn!(
                                "If the device now fails to enumerate, try holding down the button while plugging the device in order to enter the bootloader."
                            );
                            ErrorKind::DeviceDisconnectDuringOperation.error_from(source)
                        }
                        _ => source.into(),
                    }
                })?;


        } else {
            res?;
        }

        info!("Flash complete!");

        Ok(())
    }


    /// Consume the structure and retrieve its parts.
    #[allow(dead_code)]
    pub fn into_inner_parts(self) -> (UsbDevice, UsbHandle, DfuOperatingMode)
    {
        (
            self.device.into_inner().expect("Unreachable: self.device is None"),
            self.handle.into_inner().expect("Unreachable: self.handle is None"),
            self.mode
        )
    }
}

impl Display for BmpDevice
{
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error>
    {
        let display_str = match self.display() {
            Ok(s) => s,
            Err(e) => {
                // Display impls are only supposed to propagate formatter IO errors, e.g.
                // from the write!() call below, not internal errors.
                // https://doc.rust-lang.org/stable/std/fmt/index.html#formatting-traits.
                error!("Error formatting BlackMagicProbeDevice: {}", e);
                S!("Unknown Black Magic Probe (error occurred fetching device details)")
            }
        };

        write!(f, "{}", display_str)?;

        Ok(())
    }
}

/// Represents a conceptual Vector Table for Armv7 processors.
pub struct Armv7mVectorTable<'b>
{
    bytes: &'b [u8],
}

impl<'b> Armv7mVectorTable<'b>
{
    fn word(&self, index: usize) -> Result<u32, TryFromSliceError>
    {
        let start = index * 4;
        let array: [u8; 4] = self.bytes[(start)..(start + 4)]
            .try_into()?;

        Ok(u32::from_le_bytes(array))
    }


    /// Construct a conceptual Armv7m Vector Table from a bytes slice.
    pub fn from_bytes(bytes: &'b [u8]) -> Self
    {
        if bytes.len() < (4 * 2) {
            panic!("Data passed is not long enough for an Armv7m Vector Table!");
        }

        Self {
            bytes,
        }
    }

    #[allow(dead_code)]
    pub fn stack_pointer(&self) -> Result<u32, TryFromSliceError>
    {
        self.word(0)
    }

    pub fn reset_vector(&self) -> Result<u32, TryFromSliceError>
    {
        self.word(1)
    }

    #[allow(dead_code)]
    pub fn exception(&self, exception_number: u32) -> Result<u32, TryFromSliceError>
    {
        self.word((exception_number + 1) as usize)
    }
}


/// Firmware types for the Black Magic Probe.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum FirmwareType
{
    /// The bootloader. For native probes this is linked at 0x0800_0000
    Bootloader,
    /// The main application. For native probes this is linked at 0x0800_2000.
    Application,
}

impl FirmwareType
{
    /// Detect the kind of firmware from the given binary by examining its reset vector address.
    ///
    /// This function panics if `firmware.len() < 8`.
    pub fn detect_from_firmware(platform: BmpPlatform, firmware: &[u8]) -> Result<Self, Error>
    {
        let buffer = &firmware[0..(4 * 2)];

        let vector_table = Armv7mVectorTable::from_bytes(buffer);
        let reset_vector = vector_table.reset_vector()
            .map_err(|e| ErrorKind::InvalidFirmware(Some(S!("vector table too short"))).error_from(e))?;

        debug!("Detected reset vector in firmware file: 0x{:08x}", reset_vector);

        // Sanity check.
        if (reset_vector & 0x0800_0000) != 0x0800_0000 {
            return Err(ErrorKind::InvalidFirmware(Some(format!(
                "firmware reset vector seems to be outside of reasonable bounds: 0x{:08x}",
                reset_vector,
            ))).error());
        }

        let app_start = platform.load_address(Self::Application);

        if reset_vector > app_start {
            Ok(Self::Application)
        } else {
            Ok(Self::Bootloader)
        }
    }
}

impl Display for FirmwareType
{
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result
    {
        match self {
            Self::Bootloader => write!(f, "bootloader")?,
            Self::Application => write!(f, "application")?,
        };

        Ok(())
    }
}

/// Defaults to [`FirmwareType::Application`].
impl Default for FirmwareType
{
    /// Defaults to [`FirmwareType::Application`].
    fn default() -> Self
    {
        FirmwareType::Application
    }
}


/// File formats that Black Magic Probe firmware can be in.
pub enum FirmwareFormat
{
    /// Raw binary format. Made with `objcopy -O binary`. Typical file extension: `.bin`.
    Binary,

    /// The Unix ELF executable binary format. Typical file extension: `.elf`.
    Elf,

    /// Intel HEX. Typical file extensions: `.hex`, `.ihex`.
    IntelHex,
}

impl FirmwareFormat
{

    /// Detect the kind of firmware from its data.
    ///
    /// Panics if `firmware.len() < 4`.
    pub fn detect_from_firmware(firmware: &[u8]) -> Self
    {
        if &firmware[0..4] == b"\x7fELF" {
            FirmwareFormat::Elf
        } else if &firmware[0..1] == b":" {
            FirmwareFormat::IntelHex
        } else {
            FirmwareFormat::Binary
        }
    }
}



#[derive(Debug, Clone, Default)]
pub struct BmpMatcher
{
    index: Option<usize>,
    serial: Option<String>,
    port: Option<String>,
}
impl BmpMatcher
{
    pub fn new() -> Self
    {
        Default::default()
    }

    pub(crate) fn from_cli_args(matches: &ArgMatches) -> Self
    {
        Self::new()
            .index(matches.value_of("index").map(|arg| usize::from_str(arg).unwrap()))
            .serial(matches.value_of("serial_number"))
            .port(matches.value_of("port"))
    }

    /// Set the index to match against.
    #[must_use]
    pub fn index(mut self, idx: Option<usize>) -> Self
    {
        self.index = idx;
        self
    }

    /// Set the serial number to match against.
    #[must_use]
    pub fn serial<'s, IntoOptStrT>(mut self, serial: IntoOptStrT) -> Self
        where IntoOptStrT: Into<Option<&'s str>>
    {
        self.serial = serial.into().map(|s| s.to_string());
        self
    }

    /// Set the port path to match against.
    #[must_use]
    pub fn port<'s, IntoOptStrT>(mut self, port: IntoOptStrT) -> Self
        where IntoOptStrT: Into<Option<&'s str>>
    {
        self.port = port.into().map(|s| s.to_string());
        self
    }

    /// Get any index previously set with `.index()`.
    #[allow(dead_code)]
    pub fn get_index(&self) -> Option<usize>
    {
        self.index
    }

    /// Get any serial number previously set with `.serial()`.
    #[allow(dead_code)]
    pub fn get_serial(&self) -> Option<&str>
    {
        self.serial.as_deref()
    }

    /// Get any port path previously set with `.port()`.
    #[allow(dead_code)]
    pub fn get_port(&self) -> Option<&str>
    {
        self.port.as_deref()
    }

    /// Find all connected Black Magic Probe devices that match from the command-line criteria.
    ///
    /// This uses the `serial_number`, `index`, and `port` values from `matches`, treating any that
    /// were not provided as always matching.
    ///
    /// This function returns all found devices and all errors that occurred during the search.
    /// This is so errors are not hidden, but also do not prevent matching devices from being found.
    /// However, if the length of the error `Vec` is not 0, you should consider the results
    /// potentially incomplete.
    ///
    /// The `index` matcher *includes* devices that errored when attempting to match them.
    pub fn find_matching_probes(&self) -> BmpMatchResults
    {
        let mut results = BmpMatchResults {
            found: Vec::new(),
            filtered_out: Vec::new(),
            errors: Vec::new(),
        };

        let context = match rusb::Context::new() {
            Ok(c) => c,
            Err(e) => {
                results.errors.push(e.into());
                return results;
            },
        };

        let devices = match context.devices() {
            Ok(d) => d,
            Err(e) => {
                results.errors.push(e.into());
                return results;
            },
        };

        // Filter out devices that don't match the Black Magic Probe's vid/pid in the first place.
        let devices = devices
            .iter()
            .filter(|dev| {
                let desc = dev.device_descriptor()
                    .expect(libusb_cannot_fail!("libusb_get_device_descriptor()"));

                let (vid, pid) = (desc.vendor_id(), desc.product_id());
                BmpPlatform::from_vid_pid(Vid(vid), Pid(pid)).is_some()
            });

        for (index, dev) in devices.enumerate() {

            // Note: the control flow in this function is kind of weird, due to the lack of early returns
            // (since we're returning all successes and errors).

            // If we're trying to match against a serial number, we need to open the device.
            let handle = if self.serial.is_some() {
                match dev.open() {
                    Ok(h) => Some(h),
                    Err(e) => {
                        results.errors.push(e.into());
                        continue;
                    },
                }
            } else {
                None
            };

            // If we opened the device and now have that handle, try to get the device's first language, which we need
            // to request the string descriptor that contains the serial number.
            let lang = if let Some(handle) = handle.as_ref() {
                match handle.read_languages(Duration::from_secs(2)) {
                    Ok(mut l) => Some(l.remove(0)),
                    Err(e) => {
                        results.errors.push(e.into());
                        continue;
                    }
                }
            } else {
                None
            };

            // And finally, if we have successfully read that language, read and match the serial number.
            let serial_matches = if let Some(lang) = lang {
                let handle = handle.unwrap();
                let desc = dev.device_descriptor()
                    .expect(libusb_cannot_fail!("libusb_get_device_descriptor"));
                match handle.read_serial_number_string(lang, &desc, Duration::from_secs(2)) {
                    Ok(s) => Some(s) == self.serial,
                    Err(e) => {
                        results.errors.push(e.into());
                        continue;
                    },
                }
            } else if self.serial.is_none() {
                // If no serial number was specified, treat as matching.
                true
            } else {
                // If we can't get the serial number because of previous errors, treat as non-matching.
                false
            };

            // Consider the index to match if it equals that of the device or if one was not specified at all.
            let index_matches = self.index.map_or(true, |needle| needle == index);

            // Consider the port to match if it equals that of the device or if one was not specified at all.
            let port_matches = self.port.as_ref().map_or(true, |p| {
                let port_chain = dev
                    .port_numbers()
                    // Unwrap should be safe as the only possible error from libusb_get_port_numbers()
                    // is LIBUSB_ERROR_OVERFLOW, and only if the buffer given to it is too small,
                    // but rusb g ives it a buffer big enough for the maximum hub chain allowed by the spec.
                    .expect("Could not get port numbers! Hub depth > 7 shouldn't be possible!")
                    .into_iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<String>>()
                    .as_slice()
                    .join(".");

                let port_path = format!("{}-{}", dev.bus_number(), port_chain);

                p == &port_path
            });

            // Finally, check the provided matchers.
            if index_matches && port_matches && serial_matches {
                match BmpDevice::from_usb_device(dev) {
                    Ok(bmpdev) => results.found.push(bmpdev),
                    Err(e) => {
                        results.errors.push(e);
                        continue;
                    },
                };
            } else {
                results.filtered_out.push(dev);
            }
        }


        // Now, after all this, return all the devices we found, what devices were filtered out, and any errors that
        // occured along the way.
        results
    }
}


#[derive(Debug, Default)]
pub struct BmpMatchResults
{
    pub found: Vec<BmpDevice>,
    pub filtered_out: Vec<UsbDevice>,
    pub errors: Vec<Error>,
}

impl BmpMatchResults
{
    /// Pops all found devices, handling printing error and warning cases.
    pub(crate) fn pop_all(&mut self) -> Result<Vec<BmpDevice>, Error>
    {
        if self.found.is_empty() {

            // If there was only one, print that one for the user.
            if self.filtered_out.len() == 1 {
                if let Ok(bmpdev) = BmpDevice::from_usb_device(self.filtered_out.pop().unwrap()) {
                    warn!(
                        "Matching device not found, but and the following Black Magic Probe device was filtered out: {}",
                        &bmpdev,
                    );
                } else {
                    warn!("Matching device not found but 1 Black Magic Probe device was filtered out.");
                }
                warn!("Filter arguments (--serial, --index, --port) may be incorrect.");
            } else if self.filtered_out.len() > 1 {
                warn!(
                    "Matching devices not found but {} Black Magic Probe devices were filtered out.",
                    self.filtered_out.len(),
                );
                warn!("Filter arguments (--serial, --index, --port) may be incorrect.");
            }


            if !self.errors.is_empty() {
                warn!("Device not found and errors occurred when searching for devices.");
                warn!("One of these may be why the Black Magic Probe device was not found: {:?}", self.errors.as_slice());
            }
            return Err(ErrorKind::DeviceNotFound.error());
        }

        if !self.errors.is_empty() {
            warn!("Matching device found but errors occurred when searching for devices.");
            warn!("It is unlikely but possible that the incorrect device was selected!");
            warn!("Other device errors: {:?}", self.errors.as_slice());
        }

        Ok(mem::take(&mut self.found))
    }

    /// Pops a single found device, handling printing error and warning cases.
    pub(crate) fn pop_single(&mut self, operation: &str) -> Result<BmpDevice, Error>
    {
        if self.found.is_empty() {
            if !self.filtered_out.is_empty() {
                let (suffix, verb) = if self.filtered_out.len() > 1 { ("s", "were") } else { ("", "was") };
                warn!(
                    "Matching device not found and {} Black Magic Probe device{} {} filtered out.",
                    self.filtered_out.len(),
                    suffix,
                    verb,
                );
                warn!("Filter arguments (--serial, --index, --port may be incorrect.");
            }

            if !self.errors.is_empty() {
                warn!("Device not found and errors occurred when searching for devices.");
                warn!("One of these may be why the Black Magic Probe device was not found: {:?}", self.errors.as_slice());
            }
            return Err(ErrorKind::DeviceNotFound.error());
        }

        if self.found.len() > 1 {
            error!(
                "{} operation only accepts one Black Magic Probe device, but {} were found!",
                operation,
                self.found.len()
            );
            error!("Hint: try bmputil info and revise your filter arguments (--serial, --index, --port).");
            return Err(ErrorKind::TooManyDevices.error());
        }

        if !self.errors.is_empty() {
            warn!("Matching device found but errors occurred when searching for devices.");
            warn!("It is unlikely but possible that the incorrect device was selected!");
            warn!("Other device errors: {:?}", self.errors.as_slice());
        }

        Ok(self.found.remove(0))
    }

    /// Like `pop_single()`, but does not print helpful diagnostics for edge cases.
    pub(crate) fn pop_single_silent(&mut self) -> Result<BmpDevice, Error>
    {
        if self.found.len() > 1 {
            return Err(ErrorKind::TooManyDevices.error());
        } else if self.found.is_empty() {
            return Err(ErrorKind::DeviceNotFound.error());
        }

        Ok(self.found.remove(0))
    }
}


/// Waits for a Black Magic Probe to reboot, erroring after a timeout.
///
/// This function takes a port string to attempt to keep track of a single physical device
/// across USB resets.
///
/// This would take a serial number, but serial numbers can actually change between firmware
/// versions, and thus also between application and bootloader mode, so serial number is not a
/// reliable way to keep track of a single device across USB resets.
// TODO: test how reliable the port path is on multiple platforms.
pub fn wait_for_probe_reboot(port: &str, timeout: Duration, operation: &str) -> Result<BmpDevice, Error>
{
    let silence_timeout = timeout / 2;

    let matcher = BmpMatcher {
        index: None,
        serial: None,
        port: Some(port.to_string()),
    };

    let start = Instant::now();

    let mut dev = matcher.find_matching_probes().pop_single_silent();

    while let Err(ErrorKind::DeviceNotFound) = dev.err_kind() {

        trace!("Waiting for probe reboot: {} ms", Instant::now().duration_since(start).as_millis());

        // If it's been more than the timeout length, error out.
        if Instant::now().duration_since(start) > timeout {
            error!(
                "Timed-out waiting for Black Magic Probe to re-enumerate!"
            );
            return Err(ErrorKind::DeviceReboot.error_from(dev.unwrap_err()));
        }

        // Wait 200 milliseconds between checks. Hardware is a bottleneck and we
        // don't need to peg the CPU waiting for it to come back up.
        // TODO: make this configurable and/or optimize?
        thread::sleep(Duration::from_millis(200));

        // If we've been trying for over half the full timeout, start logging warnings.
        if Instant::now().duration_since(start) > silence_timeout {
            dev = matcher.find_matching_probes().pop_single(operation);
        } else {
            dev = matcher.find_matching_probes().pop_single_silent();
        }
    }

    let dev = dev?;

    Ok(dev)
}


/// Represents the firmware in use on a device that's supported.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum BmpPlatform
{
    /// Probes using the in-repo bootloader
    BlackMagicDebug,
    /// Probes using dragonBoot as an alternative bootloader
    DragonBoot,
    /// Probes using the STM32 built-in DFU bootloader
    STM32DeviceDFU,
}

impl BmpPlatform
{
    pub const BMD_RUNTIME_VID_PID: (Vid, Pid) = (Vid(0x1d50), Pid(0x6018));
    pub const BMD_DFU_VID_PID:     (Vid, Pid) = (Vid(0x1d50), Pid(0x6017));
    pub const DRAGON_BOOT_VID_PID: (Vid, Pid) = (Vid(0x1209), Pid(0xbadb));
    pub const STM32_DFU_VID_PID:   (Vid, Pid) = (Vid(0x0483), Pid(0xdf11));

    pub const fn from_vid_pid(vid: Vid, pid: Pid) -> Option<(Self, DfuOperatingMode)>
    {
        // TODO: in the case that we need to do IO to figure out the platform, this function will need
        // to be refactored to something like `from_usb_device(dev: &UsbDevice)`, and the other
        // functions of this struct will probably need to become non-const, which is fine.

        use BmpPlatform::*;
        use DfuOperatingMode::*;

        match (vid, pid) {
            Self::BMD_RUNTIME_VID_PID => Some((BlackMagicDebug, Runtime)),
            Self::BMD_DFU_VID_PID => Some((BlackMagicDebug, FirmwareUpgrade)),
            Self::DRAGON_BOOT_VID_PID => Some((DragonBoot, FirmwareUpgrade)),
            Self::STM32_DFU_VID_PID => Some((STM32DeviceDFU, FirmwareUpgrade)),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub const fn runtime_ids(self) -> (Vid, Pid)
    {
        Self::BMD_RUNTIME_VID_PID
    }

    #[allow(dead_code)]
    pub const fn dfu_ids(self) -> (Vid, Pid)
    {
        use BmpPlatform::*;

        match self {
            BlackMagicDebug => Self::BMD_DFU_VID_PID,
            DragonBoot => Self::DRAGON_BOOT_VID_PID,
            STM32DeviceDFU => Self::STM32_DFU_VID_PID,
        }
    }

    #[allow(dead_code)]
    pub const fn ids_for_mode(self, mode: DfuOperatingMode) -> (Vid, Pid)
    {
        use DfuOperatingMode::*;

        match mode {
            Runtime => self.runtime_ids(),
            FirmwareUpgrade => self.dfu_ids(),
        }
    }

    /// Get the load address for firmware of `firm_type` on this platform.
    pub const fn load_address(self, firm_type: FirmwareType) -> u32
    {
        use BmpPlatform::*;
        use FirmwareType::*;

        match self {
            BlackMagicDebug => match firm_type {
                Bootloader => 0x0800_0000,
                Application => 0x0800_2000,
            },
            DragonBoot => 0x0800_2000,
            STM32DeviceDFU => 0x0800_0000,
        }
    }
}

/// Defaults to [`BmpPlatform::BlackMagicDebug`].
impl Default for BmpPlatform
{
    /// Defaults to [`BmpPlatform::BlackMagicDebug`].
    fn default() -> Self
    {
        BmpPlatform::BlackMagicDebug
    }
}
