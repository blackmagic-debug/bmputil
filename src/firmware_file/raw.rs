// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::fs::File;
use std::io::Read;

use color_eyre::eyre::{Report, Result, eyre};
use log::debug;

use super::FirmwareStorage;

pub struct RawFirmwareFile
{
	contents: Box<[u8]>,
}

impl TryFrom<File> for RawFirmwareFile
{
	type Error = Report;

	fn try_from(mut file: File) -> Result<Self>
	{
		debug!("Loading file as raw firmware binary");
		// Pull out the entire file contents into memory and stuff it in a vec
		let mut contents = Vec::new();
		file.read_to_end(&mut contents)?;

		// Check that the result isn't too insanely big
		if contents.len() > u32::MAX as usize {
			Err(eyre!(
				"Firmware file size exceeds the max value for a 32-bit integer! Firmware binary invalid (too big)"
			))
		} else {
			// Otherwise put the vec inside our little container and be done
			Ok(Self {
				contents: contents.into_boxed_slice(),
			})
		}
	}
}

impl FirmwareStorage for RawFirmwareFile
{
	fn load_address(&self) -> Option<u32>
	{
		None
	}

	fn firmware_data(&self) -> &[u8]
	{
		&self.contents
	}
}
