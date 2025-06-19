// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::path::PathBuf;

use color_eyre::eyre::Result;

use crate::bmp::BmpDevice;
use crate::serial::bmd_rsp::BmdRspInterface;
use crate::serial::gdb_rsp::GdbRspInterface;

pub struct ProbeInterface
{
	serial_number: String,
}

impl ProbeInterface
{
	pub fn from_device(probe: BmpDevice) -> Result<Self>
	{
		Ok(Self {
			serial_number: probe.serial_number()?.to_string(),
		})
	}

	pub fn gdb_interface(&self) -> Result<GdbRspInterface>
	{
		GdbRspInterface::from_path(&self.probe_interface()?)
	}

	pub fn bmd_interface(&self) -> Result<BmdRspInterface>
	{
		BmdRspInterface::from_path(&self.probe_interface()?)
	}
}

#[cfg(any(target_os = "linux", target_os = "android"))]
impl ProbeInterface
{
	const BMD_IDSTRING_1BITSQUARED: &str = "usb-1BitSquared_Black_Magic_Probe";
	const BMD_IDSTRING_BLACKMAGIC: &str = "usb-Black_Magic_Debug_Black_Magic_Probe";
	const BMD_IDSTRING_BLACKSHERE: &str = "usb-Black_Sphere_Technologies_Black_Magic_Probe";
	const DEVICE_BY_ID: &str = "/dev/serial/by-id";

	/// Locate the GDB serial interface associated with the probe of the given serial number
	fn probe_interface(&self) -> Result<PathBuf>
	{
		use std::fs::read_dir;

		use color_eyre::eyre::eyre;

		// Start by opening the by-id serial interfaces device tree
		let dir = read_dir(Self::DEVICE_BY_ID)?;
		// Read through all the entries and try to locate one that has a serial number match
		for entry in dir {
			let entry = entry?;
			// Try to convert this entry's file name to a regular string - if we can't, it cannot be
			// a BMD serial interface (ours strictly convert to valid UTF-8)
			let file_name = entry.file_name();
			let file_name = if let Some(path) = file_name.to_str() {
				path
			} else {
				continue;
			};

			// Check to see if this entry represents a BMD based probe
			if !Self::device_is_bmd_gdb_port(file_name) {
				continue;
			}
			// It does! Horray, now check if we have an entry with a matching serial number
			if self.serial_matches(file_name) {
				// We have a match! Convert the entry into a path and return then
				return Ok(entry.path());
			}
		}
		// If we manage to get here, we could not find a matching device - so fail accordingly
		Err(eyre!("Failed to locate a device matching serial number {}", self.serial_number))
	}

	fn device_is_bmd_gdb_port(file_name: &str) -> bool
	{
		// Check if the device file name fragment starts with one of the known
		// by-id prefixes and ends with the right interface suffix
		(file_name.starts_with(Self::BMD_IDSTRING_BLACKSHERE) ||
			file_name.starts_with(Self::BMD_IDSTRING_BLACKMAGIC) ||
			file_name.starts_with(Self::BMD_IDSTRING_1BITSQUARED)) &&
			file_name.ends_with("-if00")
	}

	fn serial_matches(&self, file_name: &str) -> bool
	{
		// Start by trying to find the last _ just before the serial string
		let last_underscore = if let Some(pos) = file_name.rfind('_') {
			pos
		} else {
			return false;
		};
		// Having done that, extract the slice representing the serial number for this device
		let begin = last_underscore + 1;
		// This represents one past the last byte of the serial number string, chopping off `-if00`
		let end = file_name.len() - 5;
		// Create the slice and compare to the stored serial number
		file_name[begin..end] == self.serial_number
	}
}

#[cfg(target_os = "windows")]
impl ProbeInterface
{
	const PRODUCT_ID_BMP: u16 = 0x6018;
	const VENDOR_ID_BMP: u16 = 0x1d50;

	/// Locate the GDB serial interface associated with the probe of the given serial number
	fn probe_interface(&self) -> Result<PathBuf>
	{
		// Try to locate the probe's instance ID from the registry
		let serial_path = format!("\\{}", self.serial_number);
		let prefix = Self::read_key_from_path(&serial_path, "ParentIdPrefix")?;
		// Having grabbed the instance ID, read out the device path that matches up
		// for interface 0, giving us a `\\.\COMn` name to use with the file APIs
		let parameter_path = format!("&MI_00\\{}&0000\\Device Parameters", prefix);
		let port_name = Self::read_key_from_path(&parameter_path, "PortName")?;
		// Return by converting the string to a PathBuf
		if port_name.starts_with("\\\\.\\") {
			Ok(port_name.into())
		} else {
			Ok(format!("\\\\.\\{}", port_name).into())
		}
	}

	fn read_key_from_path(subpath: &str, key_name: &str) -> Result<String>
	{
		use log::debug;
		use windows_registry::LOCAL_MACHINE;

		// Try to open the registry subkey that contains the probe's enumeration data
		let key_path = format!(
			"SYSTEM\\CurrentControlSet\\Enum\\USB\\VID_{:04X}&PID_{:04X}{}",
			Self::VENDOR_ID_BMP,
			Self::PRODUCT_ID_BMP,
			subpath,
		);
		debug!("Trying to open registry key {} to get value {}", key_path, key_name);
		let key_handle = LOCAL_MACHINE.open(&key_path)?;

		// Now extract the data for the associated key we're interested in here -
		// with that value read, we're done and can return
		Ok(key_handle.get_string(key_name)?)
	}
}

#[cfg(target_os = "macos")]
impl ProbeInterface
{
	/// Locate the GDB serial interface associated with the probe of the given serial number
	fn probe_interface(&self) -> Result<PathBuf>
	{
		Ok(format!("/dev/cu.usbmodem{}1", self.serial_number).into())
	}
}
