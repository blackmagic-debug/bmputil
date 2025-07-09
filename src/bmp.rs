// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
// SPDX-FileContributor: Modified by Rachel Mant <git@dragonmux.network>

use std::array::TryFromSliceError;
use std::cell::{Ref, RefCell};
use std::fmt::{self, Debug, Display, Formatter};
use std::io::Read;
use std::thread;
use std::time::{Duration, Instant};

use clap::ValueEnum;
use clap::builder::PossibleValue;
use color_eyre::eyre::{Context, Error, OptionExt, Result, eyre};
use dfu_core::{DfuIo, DfuProtocol, Error as DfuCoreError, State as DfuState};
use dfu_nusb::{DfuNusb, DfuSync, Error as DfuNusbError};
use log::{debug, error, trace, warn};
use nusb::descriptors::Descriptor;
use nusb::transfer::{Control, ControlType, Direction, Recipient, TransferError};
use nusb::{Device, DeviceInfo, Interface};

pub use crate::bmp_matcher::BmpMatcher;
use crate::error::ErrorKind;
use crate::probe_identity::ProbeIdentity;
use crate::serial::bmd_rsp::BmdRspInterface;
use crate::serial::gdb_rsp::GdbRspInterface;
use crate::serial::interface::ProbeInterface;
use crate::usb::{
	DfuFunctionalDescriptor, DfuOperatingMode, DfuRequest, GenericDescriptorRef, InterfaceClass, InterfaceSubClass,
	Pid, PortId, Vid,
};

/// Semantically represents a Black Magic Probe USB device.
pub struct BmpDevice
{
	device_info: Option<DeviceInfo>,
	device: Option<Device>,

	/// Device descriptor details - string descriptor numbers for various information
	product_string_idx: u8,
	serial_string_idx: u8,
	language: u16,
	interface: Option<Interface>,

	/// The operating mode (application or DFU) the BMP is currently in.
	mode: DfuOperatingMode,

	/// The platform this BMP is running on.
	platform: BmpPlatform,

	/// RefCell for interior-mutability-based caching.
	serial: RefCell<Option<String>>,

	/// RefCell for interior-mutability-based caching.
	port: PortId,
}

fn request_type(direction: Direction, control_type: ControlType, recipient: Recipient) -> u8
{
	((direction as u8) << 7) | ((control_type as u8) << 5) | (recipient as u8)
}

fn handle_detach_errors<T>(result: std::result::Result<T, DfuNusbError>) -> Result<()>
{
	if let Err(err) = result {
		match err {
			DfuNusbError::Transfer(error) => match error {
				// If the error reported on Linux was a disconnection, that was just the
				// bootloader rebooting and we can safely ignore it
				#[cfg(any(target_os = "linux", target_os = "android", target_os = "windows"))]
				TransferError::Disconnected => Ok(()),
				// If the error reported was a STALL, that was just the
				// bootloader rebooting and we can safely ignore it
				TransferError::Stall => Ok(()),
				// If the error reported on macOS was unknown, this is most probably just the
				// OS having a bad time tracking the result of the detach packet and the
				// device rebooting as a result, so we can safely ignore it
				#[cfg(target_os = "macos")]
				TransferError::Unknown => Ok(()),
				_ => {
					warn!("Possibly spurious error from OS while rebooting probe: {}", err);
					Err(err.into())
				},
			},
			_ => {
				warn!("Possibly spurious error from OS while rebooting probe: {}", err);
				Err(err.into())
			},
		}
	} else {
		Ok(())
	}
}

impl BmpDevice
{
	pub fn from_usb_device(device_info: DeviceInfo) -> Result<Self>
	{
		// Extract the VID:PID for the device and make sure it's valid
		let vid = Vid(device_info.vendor_id());
		let pid = Pid(device_info.product_id());
		let (platform, mode) = BmpPlatform::from_vid_pid(vid, pid).ok_or_else(|| {
			warn!("Device passed to BmpDevice::from_usb_device() does not seem to be a BMP device!");
			warn!("The logic for finding this device is probably incorrect!");
			ErrorKind::DeviceNotFound.error()
		})?;

		// Try to open the device for use
		let device = device_info.open()?;

		// Extract the device descriptor and pull the string descriptor IDs for our use
		let device_desc = device.get_descriptor(
			1, // Device descriptor
			0,
			0,
			Duration::from_secs(2),
		)?;
		let device_desc = match Descriptor::new(device_desc.as_slice()) {
			None => {
				return Err(ErrorKind::DeviceSeemsInvalid("no usable device descriptor".into())
					.error()
					.into());
			},
			Some(descriptor) => descriptor,
		};

		// Now see what languages are supported
		let mut languages = device.get_string_descriptor_supported_languages(Duration::from_secs(2))?;

		// Try to get the first one
		let language = match languages.nth(0) {
			Some(language) => language,
			None => {
				return Err(ErrorKind::DeviceSeemsInvalid("no string descriptor languages".into())
					.error()
					.into());
			},
		};

		// Loop through the interfaces in this configuraiton and try to find the DFU interface
		let interface = device.active_configuration()?
            .interfaces()
            // For each of the possible alt modes this interface has
            .find(|interface|
                interface.alt_settings()
                    // See if the alt mode has a DFU interface defined
                    .filter(|alt_mode|
                        InterfaceClass(alt_mode.class()) == InterfaceClass::APPLICATION_SPECIFIC &&
                        InterfaceSubClass(alt_mode.subclass()) == InterfaceSubClass::DFU
                    )
                    // If there were any identified, this is a DFU interface
                    .count() > 0
            )
            // Take the remaining interface (if any) and turn it into an Interface
            .map(|interface| device.claim_interface(interface.interface_number()))
            .ok_or_else(|| ErrorKind::DeviceSeemsInvalid("could not find DFU interface".into()).error())??;

		// Make the port identification struct before we move device_info
		let port = PortId::new(&device_info);

		Ok(Self {
			device_info: Some(device_info),
			device: Some(device),
			product_string_idx: device_desc[15],
			serial_string_idx: device_desc[16],
			interface: Some(interface),
			language,
			mode,
			platform,
			serial: RefCell::new(None),
			port,
		})
	}

	/// Get the [`nusb::DeviceInfo`] associated with the connected Black Magic Probe.
	pub fn device_info(&self) -> &DeviceInfo
	{
		self.device_info
			.as_ref()
			.expect("Unreachable: self.device_info is None")
	}

	/// Get the [`nusb::Device`] associated with the connected Black Magic Probe.
	pub fn device(&self) -> &Device
	{
		self.device.as_ref().expect("Unreachable: self.device is None")
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
	pub fn serial_number(&self) -> Result<Ref<str>>
	{
		let serial = self.serial.borrow();
		if serial.is_some() {
			return Ok(Ref::map(serial, |s| s.as_deref().unwrap()));
		}
		// If we don't have a serial yet, drop this borrow so we can re-borrow
		// self.serial as mutable later.
		drop(serial);

		// Read out the serial string descriptor
		let serial =
			self.device()
				.get_string_descriptor(self.serial_string_idx, self.language, Duration::from_secs(2))?;

		// Finally, now that we have the serial number, cache it...
		*self.serial.borrow_mut() = Some(serial);

		// And return it.
		Ok(Ref::map(self.serial.borrow(), |s| s.as_deref().unwrap()))
	}

	/// Return the firmware identity of the device.
	///
	/// This is characterised by the product string which defines
	/// which kind of BMD-running thing we have and what version it runs
	pub fn firmware_identity(&self) -> Result<ProbeIdentity>
	{
		self.device()
			.get_string_descriptor(self.product_string_idx, self.language, Duration::from_secs(2))
			.map_err(|e| {
				ErrorKind::DeviceSeemsInvalid("no product string descriptor".into())
					.error_from(e)
					.into()
			})
			.and_then(|identity| identity.try_into())
	}

	/// Returns a string that represents the full port of the device, in the format of
	/// `<bus>-<port>.<subport>.<subport...>`.
	///
	/// This is theoretically reliable, but is also OS-reported, so it doesn't *have* to be, alas.
	pub fn port(&self) -> PortId
	{
		self.port.clone()
	}

	/// Return a string suitable for display to the user.
	///
	/// Note: this performs USB IO to retrieve the necessary string descriptors, if those strings
	/// have not yet been retrieved previously (and thus not yet cached).
	pub fn display(&self) -> Result<String>
	{
		let identity = self.firmware_identity()?;
		let serial = self.serial_number()?;

		Ok(format!("{}\n  Serial: {}\n  Port:  {}", identity, serial, self.port()))
	}

	/// Find and return the DFU functional descriptor and its interface number for the connected Black Magic Probe
	/// device.
	///
	/// Unfortunately this only returns the DFU interface's *number* and not the interface or
	/// descriptor itself, as there are ownership issues with that and rusb does not yet
	/// implement the proper traits (like. Clone.) for its types for this to work properly.
	///
	/// This does not execute any requests to the device, and only uses information already
	/// available from libusb's device structures.
	pub fn dfu_descriptors(&self) -> Result<(u8, DfuFunctionalDescriptor)>
	{
		// Loop through the interfaces in this configuraiton and try to find the DFU interface
		let interface = self.device().active_configuration()?
        .interfaces()
        // For each of the possible alt modes this interface has
        .find(|interface|
            interface.alt_settings()
                // See if the alt mode has a DFU interface defined
                .filter(|alt_mode|
                    InterfaceClass(alt_mode.class()) == InterfaceClass::APPLICATION_SPECIFIC &&
                    InterfaceSubClass(alt_mode.subclass()) == InterfaceSubClass::DFU
                )
                // If there were any identified, this is a DFU interface
                .count() > 0
        )
        .ok_or_else(|| ErrorKind::DeviceSeemsInvalid("could not find DFU interface".into()).error())?;
		// Extract the first alt-mode for its extra descriptors
		let dfu_interface_descriptor = interface
			.alt_settings()
			.nth(0)
			.ok_or_else(|| ErrorKind::DeviceSeemsInvalid("no DFU interfaces".into()).error())?;

		// Get the data for all the "extra" descriptors that follow the interface descriptor.
		let extra_descriptors: Vec<_> =
			GenericDescriptorRef::multiple_from_bytes(dfu_interface_descriptor.descriptors().as_bytes());

		// Iterate through all the "extra" descriptors to find the DFU functional descriptor.
		let dfu_func_desc_bytes: &[u8; DfuFunctionalDescriptor::LENGTH as usize] = extra_descriptors
            .into_iter()
            .find(|descriptor| descriptor.descriptor_type() == DfuFunctionalDescriptor::TYPE)
            .expect("DFU interface does not have a DFU functional descriptor! This shouldn't be possible!")
            .raw[0..DfuFunctionalDescriptor::LENGTH as usize]
            .try_into() // Convert &[u8] to &[u8; LENGTH].
            .unwrap(); // Unwrap fine as we already set the length two lines above.

		let dfu_func_desc = DfuFunctionalDescriptor::copy_from_bytes(dfu_func_desc_bytes)
			.map_err(|source| ErrorKind::DeviceSeemsInvalid("DFU functional descriptor".into()).error_from(source))?;

		Ok((dfu_interface_descriptor.interface_number(), dfu_func_desc))
	}

	/// Requests the device to leave DFU mode, using the DfuSe extensions.
	fn leave_dfu_mode(&mut self) -> Result<()>
	{
		debug!("Attempting to leave DFU mode...");

		// Start by opening the DFU interface
		let dfu = DfuNusb::open(
			self.device().clone(),
			self.interface
				.clone()
				.ok_or_eyre("BmpDevice does not have valid interface")?,
			0,
		)?;

		// Extract the functional descriptor
		let descriptor = dfu.functional_descriptor();

		// Ask if the bootloader is manifestation tolerant - this determines how the bootloader
		// must be asked to go back into the firmware. Manifestation tolerant bootloaders usue
		// a 0-length DFU_DNLOAD packet followed by DFU_GETSTATUS. non-tollerant require us
		// to instead issue DFU_DETACH. In both cases after this happens.. if the bootloader
		// is not marked as auto-detaching, we must issue a USB reset to complete the process.
		if descriptor.manifestation_tolerant {
			// Perform the zero-length DFU_DNLOAD request.
			dfu.write_control(
				request_type(Direction::Out, ControlType::Class, Recipient::Interface),
				DfuRequest::Dnload as u8,
				0,
				&[],
			)?;

			// Then perform a DFU_GETSTATUS request to complete the leave "request".
			let mut buf: [u8; 6] = [0; 6];
			let status = dfu.read_control(
				request_type(Direction::In, ControlType::Class, Recipient::Interface),
				DfuRequest::GetStatus as u8,
				0,
				&mut buf,
			)?;
			trace!("Device status after zero-length DNLOAD is 0x{:02x}", status);
			debug!("DFU_GETSTATUS request completed. Device should now re-enumerate into runtime mode.");
		} else {
			// Extract from the descriptors how long a deatch can take at most
			let timeout_ms = dfu.functional_descriptor().detach_timeout;
			// Send a DFU_DETACH request to ask the bootloader to go back into runtime mode
			handle_detach_errors(dfu.write_control(
				request_type(Direction::Out, ControlType::Class, Recipient::Interface),
				DfuRequest::Detach as u8,
				timeout_ms,
				&[],
			))?;
			debug!("DFU_DETACH request completed. Device should now re-enumerate into runtime mode.");
		}

		// If the device requires a reset to complete this request, perform it now
		if !dfu.functional_descriptor().will_detach {
			dfu.usb_reset()?;
		}

		self.interface = None;
		Ok(())
	}

	/// Performs a DFU_DETACH request to enter DFU mode.
	fn enter_dfu_mode(&mut self) -> Result<()>
	{
		let (iface_number, func_desc) = self.dfu_descriptors()?;

		let timeout_ms = func_desc.wDetachTimeOut;
		let request = Control {
			control_type: ControlType::Class,
			recipient: Recipient::Interface,
			request: DfuRequest::Detach as u8,
			value: timeout_ms,
			index: iface_number as u16,
		};

		handle_detach_errors(
			self.interface
				.as_ref()
				.unwrap()
				.control_out_blocking(
					request,
					&[],                    // buffer
					Duration::from_secs(1), // timeout for the request
				)
				.map_err(DfuNusbError::Transfer),
		)?;

		debug!("DFU_DETACH request completed. Device should now re-enumerate into DFU mode.");

		self.interface = None;
		Ok(())
	}

	/// Requests the Black Magic Probe device to detach, switching from DFU mode to runtime mode or vice versa. You
	/// probably want [`detach_and_enumerate`].
	///
	/// This function does not re-enumerate the device and re-initialize this structure, and thus after
	/// calling this function, the this [`BmpDevice`] instance will not be in a correct state
	/// if the device successfully detached. Further requests will fail, and functions like
	/// `dfu_descriptors()` may return now-incorrect data.
	pub fn request_detach(&mut self) -> Result<()>
	{
		use DfuOperatingMode::*;
		match self.mode {
			Runtime => self.enter_dfu_mode(),
			FirmwareUpgrade => self.leave_dfu_mode(),
		}
	}

	/// Requests the Black Magic Probe to detach, and re-initializes this struct with the new device.
	pub fn detach_and_enumerate(&mut self) -> Result<()>
	{
		// Save the port for finding the device again after.
		let port = self.port();

		self.request_detach()?;

		// Now drop the device so to clean up now it doesn't exist
		drop(self.device_info.take());
		drop(self.device.take());

		// TODO: make this sleep() timeout configurable?
		thread::sleep(Duration::from_millis(500));

		// Now try to find the device again on that same port.
		let dev = wait_for_probe_reboot(port, Duration::from_secs(5), "flash")?;

		// If we've made it here, then we have successfully re-found the device.
		// Re-initialize this structure from the new data.
		*self = dev;

		Ok(())
	}

	/// Detach the Black Magic Probe device, consuming the structure.
	///
	/// Currently there is not a way to recover this instance if this function errors.
	/// You'll just have to create another one.
	pub fn detach_and_destroy(mut self) -> Result<()>
	{
		self.request_detach()
	}

	pub fn reboot(&self, dfu_iface: DfuSync) -> Result<()>
	{
		// If the bootloader is not manifestation tolerant, we have to force matters with a DFU_DETACH
		if !dfu_iface.manifestation_tolerant() {
			handle_detach_errors(dfu_iface.detach())?;
		}
		// If the bootloader will not automatically detach, we have to force matters by doing a USB reset
		if !dfu_iface.will_detach() {
			Ok(dfu_iface.usb_reset()?)
		} else {
			Ok(())
		}
	}

	fn try_download<'r, R>(&mut self, firmware: &'r R, length: u32, dfu_iface: &mut dfu_nusb::DfuSync) -> Result<()>
	where
		&'r R: Read,
		R: ?Sized,
	{
		dfu_iface.download(firmware, length).map_err(|source| match source {
			DfuNusbError::Transfer(nusb::transfer::TransferError::Disconnected) => {
				error!("Black Magic Probe device disconnected during the flash process!");
				warn!(
					"If the device now fails to enumerate, try holding down the button while plugging the device in \
					 order to enter the bootloader."
				);
				ErrorKind::DeviceDisconnectDuringOperation.error_from(source).into()
			},
			_ => source.into(),
		})
	}

	/// Downloads firmware onto the device, switching into DFU mode automatically if necessary.
	///
	/// `progress` is a callback of the form `fn(just_written: usize)`, for callers to keep track of
	/// the flashing process.
	pub fn download<'r, R, P>(
		&mut self,
		firmware: &'r R,
		length: u32,
		firmware_type: FirmwareType,
		progress: P,
	) -> Result<DfuSync>
	where
		&'r R: Read,
		R: ?Sized,
		P: Fn(usize) + 'static,
	{
		if self.mode == DfuOperatingMode::Runtime {
			self.detach_and_enumerate().wrap_err("detaching device for download")?;
		}

		let load_address = self.platform.load_address(firmware_type);

		let dfu_dev = DfuNusb::open(
			self.device.take().expect("Must have a valid device handle"),
			self.interface.as_ref().unwrap().clone(),
			0,
		)?;

		if let DfuProtocol::Dfuse {
			..
		} = dfu_dev.protocol()
		{
			println!("Erasing flash...");
		}

		let mut dfu_iface = dfu_dev.into_sync_dfu();

		dfu_iface.with_progress(progress).override_address(load_address);

		debug!("Load address: 0x{:08x}", load_address);

		match self.try_download(firmware, length, &mut dfu_iface) {
			Err(error) => {
				if let Some(DfuNusbError::Dfu(DfuCoreError::StateError(DfuState::DfuError))) =
					error.downcast_ref::<DfuNusbError>()
				{
					warn!(
						"Device reported an error when trying to flash; going to clear status and try one more time..."
					);

					thread::sleep(Duration::from_millis(250));

					let request = Control {
						control_type: ControlType::Class,
						recipient: Recipient::Interface,
						request: DfuRequest::ClrStatus as u8,
						value: 0,
						index: 0, // iface number
					};

					self.interface
						.as_ref()
						.unwrap()
						.control_out_blocking(request, &[], Duration::from_secs(2))?;

					self.try_download(firmware, length, &mut dfu_iface)
				} else {
					Err(error)
				}
			},
			result => result,
		}?;

		Ok(dfu_iface)
	}

	/// Consume the structure and retrieve its parts.
	pub fn into_inner_parts(self) -> (DeviceInfo, Device, DfuOperatingMode)
	{
		(
			self.device_info.expect("Unreachable: self.device_info is None"),
			self.device.expect("Unreachable: self.device is None"),
			self.mode,
		)
	}

	/// Locate and return the GDB remote serial interface of the probe for probe debug communciations
	pub fn gdb_serial_interface(&self) -> Result<GdbRspInterface>
	{
		let serial_interface = ProbeInterface::from_device(self)?;
		serial_interface.gdb_interface()
	}

	/// Locate and return the BMD remote serial interface of the probe for probe debug communciations
	pub fn bmd_serial_interface(&self) -> Result<BmdRspInterface>
	{
		let serial_interface = ProbeInterface::from_device(self)?;
		serial_interface.bmd_interface()
	}
}

impl Debug for BmpDevice
{
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result
	{
		writeln!(f, "BmpDevice {{")?;
		writeln!(f, "    {:?}", self.device_info)?;
		writeln!(f, "    {:?}", self.mode)?;
		writeln!(f, "    {:?}", self.platform)?;
		writeln!(f, "    {:?}", self.serial)?;
		writeln!(f, "    {:?}", self.port)?;
		writeln!(f, "}}")?;

		Ok(())
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
				"Unknown Black Magic Probe (error occurred fetching device details)".into()
			},
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
		let array: [u8; 4] = self.bytes[(start)..(start + 4)].try_into()?;

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

	pub fn stack_pointer(&self) -> Result<u32, TryFromSliceError>
	{
		self.word(0)
	}

	pub fn reset_vector(&self) -> Result<u32, TryFromSliceError>
	{
		self.word(1)
	}

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
	pub fn detect_from_firmware(platform: BmpPlatform, firmware: &[u8]) -> Result<Self>
	{
		let buffer = &firmware[0..(4 * 2)];

		let vector_table = Armv7mVectorTable::from_bytes(buffer);
		let reset_vector = vector_table
			.reset_vector()
			.wrap_err("Firmware file does not seem valid: vector table too short")?;

		debug!("Detected reset vector in firmware file: 0x{:08x}", reset_vector);

		// Sanity check.
		if (reset_vector & 0x0800_0000) != 0x0800_0000 {
			return Err(eyre!(
				"Firmware file does not seem valid: reset vector address seems to be outside of reasonable bounds - \
				 0x{:08x}",
				reset_vector
			));
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

impl ValueEnum for FirmwareType
{
	fn value_variants<'a>() -> &'a [Self]
	{
		&[Self::Application, Self::Bootloader]
	}

	fn to_possible_value(&self) -> Option<PossibleValue>
	{
		match self {
			Self::Bootloader => Some("bootloader".into()),
			Self::Application => Some("application".into()),
		}
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

/// Waits for a Black Magic Probe to reboot, erroring after a timeout.
///
/// This function takes a port identifier to attempt to keep track of a single physical device
/// across USB resets.
///
/// This would take a serial number, but serial numbers can actually change between firmware
/// versions, and thus also between application and bootloader mode, so serial number is not a
/// reliable way to keep track of a single device across USB resets.
// TODO: test how reliable the port path is on multiple platforms.
pub fn wait_for_probe_reboot(port: PortId, timeout: Duration, operation: &str) -> Result<BmpDevice>
{
	let silence_timeout = timeout / 2;

	let matcher = BmpMatcher::new_with_port(port);

	let start = Instant::now();

	let mut dev = matcher.find_matching_probes().pop_single_silent();

	while let Err(ErrorKind::DeviceNotFound) = dev {
		trace!(
			"Waiting for probe reboot: {} ms",
			Instant::now().duration_since(start).as_millis()
		);

		// If it's been more than the timeout length, error out.
		if Instant::now().duration_since(start) > timeout {
			error!("Timed-out waiting for Black Magic Probe to re-enumerate!");
			return dev.map_err(|kind| {
				Error::from(kind.error())
					.wrap_err("Black Magic Probe device did not come back online (invalid firmware?)")
			});
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

	let dev = dev.map_err(|kind| kind.error())?;

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
	pub const BMD_DFU_VID_PID: (Vid, Pid) = (Vid(0x1d50), Pid(0x6017));
	pub const BMD_RUNTIME_VID_PID: (Vid, Pid) = (Vid(0x1d50), Pid(0x6018));
	pub const DRAGON_BOOT_VID_PID: (Vid, Pid) = (Vid(0x1209), Pid(0xbadb));
	pub const STM32_DFU_VID_PID: (Vid, Pid) = (Vid(0x0483), Pid(0xdf11));

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
