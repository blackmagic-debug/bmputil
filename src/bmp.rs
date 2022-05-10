use std::time::Duration;

use crate::BmputilError;
use crate::usb::{DfuFunctionalDescriptor, InterfaceClass, InterfaceSubClass, GenericDescriptorRef, DfuRequest};
use crate::usb::{Vid, Pid, DfuOperatingMode, DfuMatch};

use log::{trace, info, warn};
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
}

impl BlackmagicProbeDevice
{
	pub const VID: Vid = BmpVidPid::VID;
	pub const PID_RUNTIME: Pid = BmpVidPid::PID_RUNTIME;
	pub const PID_DFU: Pid = BmpVidPid::PID_DFU;

	/// Creates a [`BlackmagicProbeDevice`] struct from the first found connected Blackmagic Probe.
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

			let desc = dev.device_descriptor()?;
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

		let desc = device.device_descriptor()?;
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
		})
	}

	pub fn from_usb_device(device: UsbDevice) -> Result<Self, BmputilError>
	{
		let desc = device.device_descriptor()?;
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
			request_type,
			DfuRequest::GetStatus as u8,
			0, // wValue
			iface_number as u16, // wIndex
			&mut buf,
			Duration::from_secs(2),
		)?;

		trace!("Device status after zero-length DNLOAD is {:?}", status);
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
	///
	/// This consumes the struct if it succeeds, as the device will disconnect and re-enumerate
	/// in the other mode. If it fails, however, this function returns self in the Err variant.
	pub fn request_detach(mut self) -> Result<(), (Self, BmputilError)>
	{
		use DfuOperatingMode::*;
		let res = match self.mode {
			Runtime => self.enter_dfu_mode(),
			FirmwareUpgrade => self.leave_dfu_mode(),
		};
		match res {
			Ok(()) => (),
			Err(e) => return Err((self, e)),
		};

		// FIXME: This should check if the device successfully re-enumerated,
		// and possibly re-create this structure if it has.
		Ok(())
	}

    /// Consume the structure and retrieve its parts.
	pub fn into_inner_parts(self) -> (UsbDevice, UsbHandle, DfuOperatingMode)
	{
		(self.device, self.handle, self.mode)
	}
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
    //fn from_vid_pid(&self, vid: Vid, pid: Pid) -> Option<DfuOperatingMode>
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
