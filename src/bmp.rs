use std::mem;
use std::cell::{RefCell, Ref};
use std::str::FromStr;
use std::time::Duration;
use std::fmt::{self, Display, Formatter};

use crate::{BmputilError, libusb_cannot_fail};
use crate::usb::{DfuFunctionalDescriptor, InterfaceClass, InterfaceSubClass, GenericDescriptorRef, DfuRequest};
use crate::usb::{Vid, Pid, DfuOperatingMode, DfuMatch};

use log::{trace, info, warn, error};
use clap::ArgMatches;
use rusb::{UsbContext, Direction, RequestType, Recipient};

type UsbDevice = rusb::Device<rusb::Context>;
type UsbHandle = rusb::DeviceHandle<rusb::Context>;


/// Semantically represents a Blackmagic Probe USB device.
#[derive(Debug, PartialEq)]
pub struct BlackmagicProbeDevice
{
    device: rusb::Device<rusb::Context>,
    handle: rusb::DeviceHandle<rusb::Context>,
    mode: DfuOperatingMode,

    /// RefCell for interior-mutability-based caching.
    serial: RefCell<Option<String>>,
}

impl BlackmagicProbeDevice
{
    pub const VID: Vid = BmpVidPid::VID;
    pub const PID_RUNTIME: Pid = BmpVidPid::PID_RUNTIME;
    pub const PID_DFU: Pid = BmpVidPid::PID_DFU;

    /// Creates a [`BlackmagicProbeDevice`] struct from the first found connected Blackmagic Probe.
    #[allow(dead_code)]
    pub fn first_found() -> Result<Self, BmputilError>
    {
        let context = rusb::Context::new()?;
        let devices = context
            .devices()
            .unwrap();
        let mut devices = devices.iter();


        // Alas, this is the probably best way until Iterator::try_find is stable.
        // (https://github.com/rust-lang/rust/issues/63178).
        let (device, vid, pid) = loop {
            let dev = devices.next().ok_or(BmputilError::DeviceNotFoundError)?;

            let desc = dev.device_descriptor()
                .expect(libusb_cannot_fail!("libusb_get_device_descriptor()"));
            let (vid, pid) = (desc.vendor_id(), desc.product_id());

            if vid == Self::VID.0 {
                match Pid(pid) {
                    Self::PID_RUNTIME | Self::PID_DFU => break (dev, vid, pid),
                    _ => continue,
                };
            }
        };

        // Unwrap fine as we've already established this vid pid pair is
        // at least *some* kind of Blackmagic Probe.
        let mode = BmpVidPid::mode_from_vid_pid(Vid(vid), Pid(pid)).unwrap();

        let handle = device.open()?;

        Ok(Self {
            device,
            mode,
            handle,
            serial: RefCell::new(None),
        })
    }

    /// Creates a [`BlackmagicProbeDevice`] from a supplied matcher function.
    ///
    /// `matcher_fn` is a `fn(device: &rusb::Device<rusb::Context>) -> bool`. This function will
    /// This function will call `matcher_fn` with devices from libusb's device list,
    /// using the first device for which `matcher_fn` returns `true`.
    /// However, it is not assumed that `matcher_fn` is in fact a valid Blackmagic Probe device.
    /// If the device matched by `matcher_fn` does not seem to be a valid Blackmagic Probe device,
    /// this function returns `Err(BmputilError::DeviceNotFoundError)`.
    #[allow(dead_code)]
    pub fn from_matching<MatcherT>(matcher_fn: MatcherT) -> Result<Self, BmputilError>
        where MatcherT: Fn(&UsbDevice) -> bool,
    {
        let context = rusb::Context::new()?;
        let device = context
            .devices()
            .unwrap()
            .iter()
            .find(matcher_fn)
            .ok_or(BmputilError::DeviceNotFoundError)?;

        let desc = device.device_descriptor()
            .expect(libusb_cannot_fail!("libusb_get_device_descriptor()"));
        let (vid, pid) = (Vid(desc.vendor_id()), Pid(desc.product_id()));

        // Unlike in [`Self::first_found`] we're not unwrapping here as the supplied matcher
        // function may have actually given us a device that is not a Blackmagic Probe in
        // either mode.
        let mode = BmpVidPid::mode_from_vid_pid(vid, pid).ok_or_else(|| {
            warn!("Matcher function given to find a BMP device does not seem to have returned a BMP device!");
            warn!("The matcher function passed to BlackmagicProbeDevice::from_matching() is probably incorrect!");
            BmputilError::DeviceNotFoundError
        })?;

        let handle = device.open()?;

        Ok(Self {
            device,
            mode,
            handle,
            serial: RefCell::new(None),
        })
    }

    pub fn from_usb_device(device: UsbDevice) -> Result<Self, BmputilError>
    {
        let desc = device.device_descriptor()
            .expect(libusb_cannot_fail!("libusb_get_device_descriptor()"));
        let (vid, pid) = (Vid(desc.vendor_id()), Pid(desc.product_id()));
        let mode = BmpVidPid::mode_from_vid_pid(vid, pid).ok_or_else(|| {
            warn!("Device passed to BlackmagicProbeDevice::from_usb_device() does not seem to be a BMP device!");
            warn!("The logic for finding this device is probably incorrect!");
            BmputilError::DeviceNotFoundError
        })?;

        let handle = device.open()?;


        Ok(Self {
            device,
            mode,
            handle,
            serial: RefCell::new(None),
        })
    }

    /// Get the [`rusb::Device<rusb::Context>`] associated with the connected Blackmagic Probe.
    #[allow(dead_code)]
    pub fn device(&mut self) -> &UsbDevice
    {
        &self.device
    }

    /// Violate struct invariants if you want. I'm not the boss of you.
    #[allow(dead_code)]
    pub unsafe fn device_mut(&mut self) -> &mut UsbDevice
    {
        &mut self.device
    }

    /// Get the [`rusb::DeviceHandle<rusb::Context>`] associated with the connected Blackmagic Probe.
    #[allow(dead_code)]
    pub fn handle(&self) -> &UsbHandle
    {
        &self.handle
    }

    /// Violate struct invariants if you want. I'm not the boss of you.
    #[allow(dead_code)]
    pub unsafe fn handle_mut(&mut self) -> &mut UsbHandle
    {
        &mut self.handle
    }

    pub fn operating_mode(&self) -> DfuOperatingMode
    {
        self.mode
    }

    /// Returns a the serial number string for this device.
    ///
    /// This struct caches the serial number in an [`std::cell::RefCell`],
    /// and thus returns a `Ref<str>` rather than the `&str` directly.
    /// Feel free to clone the result if you want a directly referenceable value.
    pub fn serial_number(&self) -> Result<Ref<str>, BmputilError>
    {
        let serial = self.serial.borrow();
        if serial.is_some() {
            return Ok(Ref::map(serial, |s| s.as_deref().unwrap()));
        }
        // If we don't have a serial yet, drop this borrow so we can re-borrow
        // self.serial as mutable later.
        drop(serial);

        let languages = self.handle.read_languages(Duration::from_secs(2))?;
        if languages.is_empty() {
            return Err(BmputilError::DeviceSeemsInvalidError {
                source: None,
                invalid_thing: String::from("no string descriptor languages"),
            });
        }

        let language = languages.first().unwrap(); // Okay as we proved len > 0.

        let serial = self.handle
            .read_serial_number_string(
                *language,
                &self.device.device_descriptor().unwrap(),
                Duration::from_secs(2),
            )?;

        // Finally, now that we have the serial number, cache it...
        *self.serial.borrow_mut() = Some(serial);

        // And return it.
        Ok(Ref::map(self.serial.borrow(), |s| s.as_deref().unwrap()))
    }

    /// Find and return the DFU functional descriptor and its interface number for the connected Blackmagic Probe device.
    ///
    /// Unfortunately this only returns the DFU interface's *number* and not the interface or
    /// descriptor itself, as there are ownership issues with that and rusb does not yet
    /// implement the proper traits (like. Clone.) for its types for this to work properly.
    ///
    /// This does not execute any requests to the device, and only uses information already
    /// available from libusb's device structures.
    pub fn dfu_descriptors(&self) -> Result<(u8, DfuFunctionalDescriptor), BmputilError>
    {
        let configuration = match self.device.active_config_descriptor() {
            Ok(d) => d,
            Err(rusb::Error::NotFound) => {
                // In the unlikely even that the OS reports the device as unconfigured
                // (possibly because it was only just connected and is still enumerating?)
                // try instead to simply get the first configuration, and hope that the
                // device is configured by the time we try to send requests to it.
                // I'm not actually sure this case is even possibly on any OS, but might
                // as well check.

                warn!("OS reports Blackmagic Probe device is unconfigured!");
                warn!("Attempting to continue anyway, in case the device is still in the process of enumerating.");

                // USB configurations are 1-indexed, as 0 is considered
                // to be "unconfigured".
                match self.device.config_descriptor(1) {
                    Ok(d) => d,
                    Err(e) => {
                        return Err(BmputilError::DeviceSeemsInvalidError {
                            source: Some(e.into()),
                            invalid_thing: String::from("no configuration descriptor exists"),
                        });
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
            .ok_or_else(|| BmputilError::DeviceSeemsInvalidError {
                source: None,
                invalid_thing: String::from("no DFU interfaces"),
            })?;

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
            })?;

        Ok((dfu_interface_descriptor.interface_number(), dfu_func_desc))
    }

    /// Requests the device to leave DFU mode, using the DefuSe extensions.
    fn leave_dfu_mode(&mut self) -> Result<(), BmputilError>
    {
        let (iface_number, _func_desc) = self.dfu_descriptors()?;
        self.handle.claim_interface(iface_number)?;

        let request_type = rusb::request_type(
            Direction::Out,
            RequestType::Class,
            Recipient::Interface,
        );

        // Perform the zero-length DFU_DNLOAD request.
        let _response = self.handle.write_control(
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
        let status = self.handle.read_control(
            request_type, // bmRequestType
            DfuRequest::GetStatus as u8, // bRequest
            0, // wValue
            iface_number as u16, // wIndex
            &mut buf,
            Duration::from_secs(2),
        )?;

        trace!("Device status after zero-length DNLOAD is 0x{:02x}", status);
        info!("DFU_GETSTATUS request completed. Device should now re-enumerate into runtime mode.");


        Ok(())
    }

    /// Performs a DFU_DETACH request to enter DFU mode.
    fn enter_dfu_mode(&mut self) -> Result<(), BmputilError>
    {
        let (iface_number, func_desc) = self.dfu_descriptors()?;
        self.handle.claim_interface(iface_number)?;

        let request_type = rusb::request_type(
            Direction::Out,
            RequestType::Class,
            Recipient::Interface,
        );
        let timeout_ms = func_desc.wDetachTimeOut;

        let _response = self.handle.write_control(
            request_type, // bmpRequestType
            DfuRequest::Detach as u8, // bRequest
            timeout_ms, // wValue
            iface_number as u16, // wIndex
            &[], // buffer
            Duration::from_secs(2), // timeout for libusb
        )?;
        info!("DFU_DETACH request completed. Device should now re-enumerate into DFU mode.");

        Ok(())
    }

    /// Requests the Blackmagic Probe device to detach, switching from DFU mode to runtime mode or vice versa.
    pub fn request_detach(&mut self) -> Result<(), BmputilError>
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

        // FIXME: This should check if the device successfully re-enumerated,
        // and possibly re-create this structure if it has.
        Ok(())
    }

    /// Consume the structure and retrieve its parts.
    #[allow(dead_code)]
    pub fn into_inner_parts(self) -> (UsbDevice, UsbHandle, DfuOperatingMode)
    {
        (self.device, self.handle, self.mode)
    }
}

impl Display for BlackmagicProbeDevice
{
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error>
    {
        // FIXME: this function needs proper error handling.

        let languages = self.handle
            .read_languages(Duration::from_secs(2))
            .expect("Failed to read supported languages from Black Magic Probe device");

        let product_string = self.handle
            .read_product_string(
                *languages.first().unwrap(),
                &self.device.device_descriptor().unwrap(),
                Duration::from_secs(2),
            )
            .unwrap();

        let serial = self.serial_number()
            .expect("Failed to read serial number from Black Magic Probe device");

        let bus = self.device.bus_number();
        let path = self.device
            .port_numbers()
            .unwrap()
            .into_iter()
            .map(|v| v.to_string())
            .collect::<Vec<String>>()
            .as_slice()
            .join(".");

        write!(f, "{}\n  Serial: {}  \n", product_string, serial)?;
        write!(f, "  Port:   {}-{}", bus, path)?;

        Ok(())
    }
}


#[derive(Debug, Clone, Default)]
pub struct BlackmagicProbeMatcher
{
    index: Option<usize>,
    serial: Option<String>,
    port: Option<String>,
}
impl BlackmagicProbeMatcher
{
    pub fn new() -> Self
    {
        Default::default()
    }

    pub(crate) fn from_clap_matches(matches: &ArgMatches) -> Self
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
}


#[derive(Debug)]
pub struct BlackmagicProbeMatchResults
{
    pub found: Vec<BlackmagicProbeDevice>,
    pub filtered_out: Vec<UsbDevice>, // FIXME: rusb::Device?
    pub errors: Vec<BmputilError>,
}

impl BlackmagicProbeMatchResults
{
    /// Pops all found devices, handling printing error and warning cases.
    pub(crate) fn pop_all(&mut self) -> Result<Vec<BlackmagicProbeDevice>, BmputilError>
    {
        if self.found.is_empty() {
            if !self.filtered_out.is_empty() {
                let (suffix, verb) = if self.filtered_out.len() > 1 { ("s", "were") } else { ("", "was") };
                warn!(
                    "Matching device not found and {} Blackmagic Probe device{} {} filtered out.",
                    self.filtered_out.len(),
                    suffix,
                    verb,
                );
                warn!("Filter arguments (--serial, --index, --port may be incorrect.");
            }

            if !self.errors.is_empty() {
                warn!("Device not found and errors occurred when searching for devices.");
                warn!("One of these may be why the Blackmagic Probe device was not found: {:?}", self.errors.as_slice());
            }
            return Err(BmputilError::DeviceNotFoundError);
        }

        if !self.errors.is_empty() {
            warn!("Matching device found but errors occurred when searching for devices.");
            warn!("It is unlikely but possible that the incorrect device was selected!");
            warn!("Other device errors: {:?}", self.errors.as_slice());
        }

        return Ok(mem::take(&mut self.found));
    }

    /// Pops a single found device, handling printing error and warning cases.
    pub(crate) fn pop_single(&mut self, operation: &str) -> Result<BlackmagicProbeDevice, BmputilError>
    {
        if self.found.is_empty() {
            if !self.filtered_out.is_empty() {
                let (suffix, verb) = if self.filtered_out.len() > 1 { ("s", "were") } else { ("", "was") };
                warn!(
                    "Matching device not found and {} Blackmagic Probe device{} {} filtered out.",
                    self.filtered_out.len(),
                    suffix,
                    verb,
                );
                warn!("Filter arguments (--serial, --index, --port may be incorrect.");
            }

            if !self.errors.is_empty() {
                warn!("Device not found and errors occurred when searching for devices.");
                warn!("One of these may be why the Blackmagic Probe device was not found: {:?}", self.errors.as_slice());
            }
            return Err(BmputilError::DeviceNotFoundError);
        }

        if self.found.len() > 1 {
            error!(
                "{} operation only accepts one Blackmagic Probe device, but {} were found!",
                operation,
                self.found.len()
            );
            error!("Hint: try bmputil info and revise your filter arguments (--serial, --index, --port).");
            return Err(BmputilError::TooManyDevicesError);
        }

        if !self.errors.is_empty() {
            warn!("Matching device found but errors occurred when searching for devices.");
            warn!("It is unlikely but possible that the incorrect device was selected!");
            warn!("Other device errors: {:?}", self.errors.as_slice());
        }

        Ok(self.found.remove(0))
    }
}


/// Find all connected Blackmagic Probe devices that match from the command-line criteria.
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
pub fn find_matching_probes(matcher: &BlackmagicProbeMatcher) -> BlackmagicProbeMatchResults
{
    //let mut found: Vec<BlackmagicProbeDevice> = Vec::new();
    //let mut filtered_out: Vec<UsbDevice> = Vec::new();
    //let mut errors: Vec<BmputilError> = Vec::new();
    let mut results = BlackmagicProbeMatchResults {
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

    // Filter out devices that don't match the Blackmagic Probe's vid/pid in the first place.
    let devices = devices
        .iter()
        .filter(|dev| {
            let desc = dev.device_descriptor()
                .expect(libusb_cannot_fail!("libusb_get_device_descriptor()"));

            let (vid, pid) = (desc.vendor_id(), desc.product_id());
            BmpVidPid::mode_from_vid_pid(Vid(vid), Pid(pid)).is_some()
        });

    for (index, dev) in devices.enumerate() {

        // If we're trying to match against a serial number, try to open the device.
        let handle = if let Some(_) = matcher.serial {
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

        // If we opened the device and now have that handle, try to get the device's first
        // language.
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

        // And finally, if we have successfully read that language, read and match the serial
        // number.
        let serial_matches = if let Some(lang) = lang {
            let handle = handle.unwrap();
            let desc = dev.device_descriptor()
                .expect(libusb_cannot_fail!("libusb_get_device_descriptor()"));
            match handle.read_serial_number_string(lang, &desc, Duration::from_secs(2)) {
                Ok(s) => Some(s) == matcher.serial,
                Err(e) => {
                    results.errors.push(e.into());
                    continue;
                },
            }
        } else {
            // If we don't have a serial number, treat it as matching.
            true
        };

        // Consider the index to match if it equals that of the device or if one was not specified
        // at all.
        let index_matches = matcher.index.map_or(true, |needle| needle == index);

        // Consider the port to match if it equals that of the device or if one was not specified
        // at all.
        let port_matches = matcher.port.as_ref().map_or(true, |p| {
            let port_chain = dev
                .port_numbers()
                // Unwrap should be safe as the only possible error from libusb_get_port_numbers()
                // is LIBUSB_ERROR_OVERFLOW, and only if the buffer given to it is too small, but
                // rusb gives it a buffer big enough for the maximum hub chain allowed by the spec.
                .expect("Could not get port numbers! Hub depth > 7 shouldn't be possible!")
                .into_iter()
                .map(|v| v.to_string())
                .collect::<Vec<String>>()
                .as_slice()
                .join(".");

            let port_path = format!("{}-{}", dev.bus_number(), port_chain);

            p == &port_path
        });

        // Finally, filter devices based on whether the provided criteria match.
        if index_matches && port_matches && serial_matches {
            match BlackmagicProbeDevice::from_usb_device(dev) {
                Ok(bmpdev) => results.found.push(bmpdev),
                Err(e) => {
                    results.errors.push(e.into());
                    continue;
                },
            };
        } else {
            results.filtered_out.push(dev);
        }
    }

    // And finally, after all this, return all the devices we found, what devices were filtered
    // out, and any errors that occurred along the way.
    results
}


pub struct BmpVidPid;
impl BmpVidPid
{
    pub const VID: Vid = Vid(0x1d50);
    pub const PID_RUNTIME: Pid = Pid(0x6018);
    pub const PID_DFU: Pid = Pid(0x6017);
}
impl DfuMatch for BmpVidPid
{
    fn mode_from_vid_pid(vid: Vid, pid: Pid) -> Option<DfuOperatingMode>
    {
        match vid {
            Self::VID => {
                match pid {
                    Self::PID_RUNTIME => Some(DfuOperatingMode::Runtime),
                    Self::PID_DFU => Some(DfuOperatingMode::FirmwareUpgrade),
                    _ => None,
                }
            },
            _ => None,
        }
    }
}
