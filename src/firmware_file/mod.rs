// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::fs::File;
use std::io::{Read, Seek};
use std::path::Path;

use color_eyre::eyre::{Context, Result, eyre};

mod elf;
mod ihex;
mod raw;

use self::elf::ELFFirmwareFile;
use self::ihex::IntelHexFirmwareFile;
use self::raw::RawFirmwareFile;

trait FirmwareStorage
{
	fn load_address(&self) -> Option<u32>;
	fn firmware_data(&self) -> &[u8];
}

pub struct FirmwareFile
{
	inner: Box<dyn FirmwareStorage>,
}

impl FirmwareFile
{
	/// Construct a FirmwareFile from a path to a file
	pub fn from_path(file_name: &Path) -> Result<Self>
	{
		let mut file =
			File::open(file_name).wrap_err_with(|| eyre!("Failed to read file {} as firmware", file_name.display()))?;

		let mut signature = [0u8; 4];
		let _ = file.read(&mut signature)?;
		file.rewind()?;

		let storage: Box<dyn FirmwareStorage> = if &signature == b"\x7fELF" {
			Box::new(ELFFirmwareFile::from(file))
		} else if &signature[0..1] == b":" {
			Box::new(IntelHexFirmwareFile::from(file))
		} else {
			Box::new(RawFirmwareFile::try_from(file)?)
		};

		Ok(Self {
			inner: storage,
		})
	}

	pub fn load_address(&self) -> Option<u32>
	{
		self.inner.load_address()
	}

	/// Provides the firmware data this file holds in a format suitable for
	/// writing into Flash directly at the load address
	pub fn firmware_data(&self) -> &[u8]
	{
		self.inner.firmware_data()
	}
}
