// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
// SPDX-FileContributor: Modified by Rachel Mant <git@dragonmux.network>

use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

use color_eyre::eyre::{Context, Result, eyre};
use color_eyre::owo_colors::OwoColorize;
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, error, info, warn};

use crate::bmp::{self, BmpDevice};
use crate::firmware_file::FirmwareFile;
use crate::firmware_type::FirmwareType;
use crate::usb::PortId;
use crate::{AllowDangerous, BmpParams, FlashParams};

pub struct Firmware
{
	firmware_type: FirmwareType,
	firmware_file: FirmwareFile,
}

impl Firmware
{
	pub fn new<Params>(params: &Params, device: &BmpDevice, firmware_file: FirmwareFile) -> Result<Self>
	where
		Params: BmpParams + FlashParams,
	{
		Ok(Self {
			firmware_type: Self::determine_firmware_type(params, device, &firmware_file)?,
			firmware_file,
		})
	}

	// XXX: Move me to FirmwareFile?
	fn determine_firmware_type<Params>(
		params: &Params,
		device: &BmpDevice,
		firmware_file: &FirmwareFile,
	) -> Result<FirmwareType>
	where
		Params: BmpParams + FlashParams,
	{
		// Figure out what kind of firmware we're being asked to work with here
		// Using the platform to determine the link address.
		let platform = device.platform();
		let firmware_type =
			FirmwareType::detect_from_firmware(platform, firmware_file).wrap_err("detecting firmware type")?;

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
		// Extract the firmware type as a value so it can be captured and moved (copied) by the progress lambda
		let firmware_type = self.firmware_type;
		// Pull out the data to program
		let firmware_data = self.firmware_file.data();
		// Pull out the length of the firmware image and make it a u64
		let firmware_length = self.firmware_file.len() as u64;

		// We need an Rc<T> as [`dfu_core::sync::DfuSync`] requires `progress` to be 'static,
		// so it must be moved into the closure. However, since we need to call .finish() here,
		// it must be owned by both. Hence: Rc<T>.
		// Default template: `{wide_bar} {pos}/{len}`.
		let progress_bar = ProgressBar::new(firmware_length).with_style(
			ProgressStyle::default_bar()
				.template(" {percent:>3}% |{bar:50}| {bytes}/{total_bytes} [{binary_bytes_per_sec} {elapsed}]")
				.unwrap(),
		);
		let progress_bar = Rc::new(progress_bar);
		let enclosed = Rc::clone(&progress_bar);

		let result = device.download(firmware_data, firmware_type, move |flash_pos_delta| {
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

		if progress_bar.position() == firmware_length {
			device.reboot(dfu_iface)
		} else {
			Err(eyre!("Failed to flash device, download incomplete"))
		}
	}
}

fn check_programming(port: PortId) -> Result<()>
{
	let dev = bmp::wait_for_probe_reboot(port, Duration::from_secs(5), "flash").inspect_err(|_| {
		error!("Black Magic Probe did not re-enumerate after flashing! Invalid firmware?");
	})?;

	// Now the device has come back, we need to see if the firmware programming cycle succeeded.
	// This starts by extracting the firmware identity string to check
	let identity = dev.firmware_identity().inspect_err(|_| {
		error!("Error reading firmware version after flash! Invalid firmware?");
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
	let firmware_file = FirmwareFile::from_path(&file_name)?;

	// Grab the the port the probe can be found on, which we need to re-find the probe after rebooting.
	let port = device.port();

	let firmware = Firmware::new(params, &device, firmware_file)?;

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
