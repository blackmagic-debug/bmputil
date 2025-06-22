// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
// SPDX-FileContributor: Modified by Rachel Mant <git@dragonmux.network>

use std::io::{Read, Write};
use std::path::PathBuf;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

use color_eyre::eyre::{Context, Result, eyre};
use color_eyre::owo_colors::OwoColorize;
use dfu_nusb::Error as DfuNusbError;
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, error, info, warn};
use nusb::transfer::TransferError;

use crate::bmp::{self, BmpDevice, FirmwareFormat, FirmwareType};
use crate::usb::PortId;
use crate::{AllowDangerous, BmpParams, FlashParams, elf};

pub struct Firmware
{
	firmware_type: FirmwareType,
	data: Vec<u8>,
	length: u32,
}

impl Firmware
{
	pub fn new<Params>(params: &Params, device: &BmpDevice, firmware_data: Vec<u8>) -> Result<Self>
	where
		Params: BmpParams + FlashParams,
	{
		let firmware_length = firmware_data.len();
		let firmware_length = u32::try_from(firmware_length)
			.expect("firmware filesize exceeded 32 bits! Firmware binary must be invalid (too big)");

		Ok(Self {
			firmware_type: Self::determine_firmware_type(params, device, &firmware_data)?,
			data: firmware_data,
			length: firmware_length,
		})
	}

	fn determine_firmware_type<Params>(
		params: &Params,
		device: &BmpDevice,
		firmware_data: &Vec<u8>,
	) -> Result<FirmwareType>
	where
		Params: BmpParams + FlashParams,
	{
		// Figure out what kind of firmware we're being asked to work with here
		// Using the platform to determine the link address.
		let platform = device.platform();
		let firmware_type =
			FirmwareType::detect_from_firmware(platform, &firmware_data).wrap_err("detecting firmware type")?;

		debug!("Firmware file was detected as {}", firmware_type);

		// But allow the user to override that type, if they *really* know what they are doing.
		let firmware_type = match params.override_firmware_type() {
			Some(override_firmware_type) => {
				match params.allow_dangerous_options() {
					AllowDangerous::Really => warn!(
						"Overriding firmware-type detection and flashing to user-specified location ({}) instead!",
						override_firmware_type
					),
					AllowDangerous::Never => {
						eprintln!(
							"{} --override-firmware-type is used to override the firmware type detection and flash a \
							 firmware binary to a location other than the one that it seems to be designed for.\nThis \
							 is a potentially destructive operation and can result in an unbootable device! (can \
							 require a second, external JTAG debugger and manual wiring to fix!)\n\nDo not use this \
							 option unless you are a firmware developer and really know what you are doing!\n\nIf you \
							 are sure this is really what you want to do, run again with \
							 --allow-dangerous-options=really",
							"WARNING:".red()
						);
						std::process::exit(1);
					},
				}
				override_firmware_type
			},
			None => firmware_type,
		};

		Ok(firmware_type)
	}

	pub fn program_firmware(&self, device: &mut BmpDevice) -> Result<()>
	{
		// We need an Rc<T> as [`dfu_core::sync::DfuSync`] requires `progress` to be 'static,
		// so it must be moved into the closure. However, since we need to call .finish() here,
		// it must be owned by both. Hence: Rc<T>.
		// Default template: `{wide_bar} {pos}/{len}`.
		let progress_bar = ProgressBar::new(self.length as u64).with_style(
			ProgressStyle::default_bar()
				.template(" {percent:>3}% |{bar:50}| {bytes}/{total_bytes} [{binary_bytes_per_sec} {elapsed}]")
				.unwrap(),
		);
		let progress_bar = Rc::new(progress_bar);
		let enclosed = Rc::clone(&progress_bar);
		// Extract the firmware type as a value so it can be captured and moved (copied) by the progress lambda
		let firmware_type = self.firmware_type;

		let result = device.download(&*self.data, self.length, firmware_type, move |flash_pos_delta| {
			// Don't actually print flashing until the erasing has finished.
			if enclosed.position() == 0 {
				if firmware_type == FirmwareType::Application {
					enclosed.println("Flashing...");
				} else {
					enclosed.println("Flashing bootloader...");
				}
			}
			enclosed.inc(flash_pos_delta as u64);
		});
		progress_bar.finish();
		let dfu_iface = result?;
		info!("Flash complete!");

		if progress_bar.position() == (self.length as u64) {
			match device.reboot(dfu_iface) {
				Err(err) => {
					let err = err.downcast::<DfuNusbError>()?;
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
								warn!("Possibly spurious error from OS at the very end of flashing: {}", err);
								Err(err.into())
							},
						},
						_ => {
							warn!("Possibly spurious error from OS at the very end of flashing: {}", err);
							Err(err.into())
						},
					}
				},
				result => result,
			}
		} else {
			Err(eyre!("Failed to flash device, download incomplete"))
		}
	}
}

fn intel_hex_error() -> !
{
	eprintln!(
		"{} The specified firmware file appears to be an Intel HEX file, but Intel HEX files are not currently \
		 supported. Please use a binary file (e.g. blackmagic.bin), or an ELF (e.g. blackmagic.elf) to flash.",
		"Error:".red()
	);
	std::process::exit(1);
}

fn read_firmware(file_name: PathBuf) -> Result<Vec<u8>>
{
	let firmware_file = std::fs::File::open(file_name.as_path())
		.wrap_err_with(|| eyre!("Failed to read firmware file {} to Flash", file_name.display()))?;

	let mut firmware_file = std::io::BufReader::new(firmware_file);

	let mut firmware_data = Vec::new();
	firmware_file.read_to_end(&mut firmware_data).unwrap();

	// FirmwareFormat::detect_from_firmware() needs at least 4 bytes, and
	// FirmwareType::detect_from_firmware() needs at least 8 bytes,
	// but also if we don't even have 8 bytes there's _no way_ this is valid firmware.
	if firmware_data.len() < 8 {
		return Err(eyre!("Firmware file appears invalid: less than 8 bytes long"));
	}

	// Extract the actual firmware data from the file, based on the format we're using.
	let firmware_data = match FirmwareFormat::detect_from_firmware(&firmware_data) {
		FirmwareFormat::Binary => firmware_data,
		FirmwareFormat::Elf => elf::extract_binary(&firmware_data)?,
		FirmwareFormat::IntelHex => intel_hex_error(), // FIXME: implement this.
	};

	Ok(firmware_data)
}

fn check_programming(port: PortId) -> Result<()>
{
	let dev = bmp::wait_for_probe_reboot(port, Duration::from_secs(5), "flash").map_err(|e| {
		error!("Black Magic Probe did not re-enumerate after flashing! Invalid firmware?");
		e
	})?;

	// Now the device has come back, we need to see if the firmware programming cycle succeeded.
	// This starts by extracting the firmware identity string to check
	let identity = dev.firmware_identity().map_err(|e| {
		error!("Error reading firmware version after flash! Invalid firmware?");
		e
	})?;

	println!(
		"Black Magic Probe successfully rebooted into firmware version {}",
		identity.version
	);

	Ok(())
}

pub fn flash_probe<Params>(params: &Params, mut device: BmpDevice, file_name: PathBuf) -> Result<()>
where
	Params: BmpParams + FlashParams,
{
	let firmware_data = read_firmware(file_name)?;

	// Grab the the port the probe can be found on, which we need to re-find the probe after rebooting.
	let port = device.port();

	let firmware = Firmware::new(params, &device, firmware_data)?;

	// If we can't get the string descriptors, try to go ahead with flashing anyway.
	// It's unlikely that other control requests will succeed, but the OS might be messing with
	// the string descriptor stuff.
	let _ = writeln!(std::io::stdout(), "Found: {}", device).map_err(|e| {
		error!(
			"Failed to read string data from Black Magic Probe: {}\nTrying to continue anyway...",
			e
		);
	});

	firmware.program_firmware(&mut device)?;

	// Programming triggers a probe reboot, so after this we have to get libusb to
	// drop the device, wait a little for the probe to go away and then wait on the probe to come back.
	drop(device);
	thread::sleep(Duration::from_millis(250));

	check_programming(port)
}
