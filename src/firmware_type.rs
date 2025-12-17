// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
// SPDX-FileContributor: Modified by Rachel Mant <git@dragonmux.network>

use std::array::TryFromSliceError;
use std::fmt::Display;

use clap::{ValueEnum, builder::PossibleValue};
use color_eyre::eyre::{Context, Result, eyre};
use log::debug;

use crate::{bmp::BmpPlatform, firmware_file::FirmwareFile};

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
	pub fn detect_from_firmware(platform: BmpPlatform, firmware_file: &FirmwareFile) -> Result<Self>
	{
        // If the firmware image has a load address
        if let Some(load_address) = firmware_file.load_address() {
            // Check if the address is the bootloader area for the platform
            let boot_start = platform.load_address(Self::Bootloader);
            return Ok(
                if load_address == boot_start {
                    Self::Bootloader
                } else {
                    Self::Application
                }
            );
        }

        // If the firmware doesn't have a known load address, fall back to figuring it out
        // from the NVIC table at the front of the image
		let buffer = &firmware_file.data()[0..(4 * 2)];

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
