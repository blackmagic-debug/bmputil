// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::fs::File;
use std::io::{Read, Seek};
use std::path::Path;

use color_eyre::eyre::{Context, Result, eyre};

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

trait FirmwareStorage
{
	fn find_load_address(&self) -> Option<u32>;
	fn firmware_data(&self) -> &[u8];
}

struct RawFirmwareFile
{
	contents: Vec<u8>,
}

struct IntelHexFirmwareFile {}

struct ELFFirmwareFile {}

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
			Box::new(RawFirmwareFile::from(file)?)
		};

		Ok(Self {
			inner: storage,
		})
	}
}

impl RawFirmwareFile
{
	fn from(mut file: File) -> Result<Self>
	{
		let mut contents = Vec::new();
		file.read_to_end(&mut contents)?;
		Ok(Self {
			contents,
		})
	}
}

impl FirmwareStorage for RawFirmwareFile
{
	fn find_load_address(&self) -> Option<u32>
	{
		None
	}

	fn firmware_data(&self) -> &[u8]
	{
		&self.contents
	}
}

impl IntelHexFirmwareFile
{
	fn from(_file: File) -> Self
	{
		Self {}
	}
}

impl FirmwareStorage for IntelHexFirmwareFile
{
	fn find_load_address(&self) -> Option<u32>
	{
		None
	}

	fn firmware_data(&self) -> &[u8]
	{
		&[]
	}
}

impl ELFFirmwareFile
{
	fn from(_file: File) -> Self
	{
		Self {}
	}
}

impl FirmwareStorage for ELFFirmwareFile
{
	fn find_load_address(&self) -> Option<u32>
	{
		None
	}

	fn firmware_data(&self) -> &[u8]
	{
		&[]
	}
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
