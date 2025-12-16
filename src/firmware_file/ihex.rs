// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::fs::File;

use owo_colors::OwoColorize;

use super::FirmwareStorage;

pub struct IntelHexFirmwareFile {}

impl From<File> for IntelHexFirmwareFile
{
	fn from(_file: File) -> Self
	{
		eprintln!(
			"{} The specified firmware file appears to be an Intel HEX file, but Intel HEX files are not currently \
			 supported. Please use a binary file (e.g. blackmagic.bin), or an ELF (e.g. blackmagic.elf) to flash.",
			"Error:".red()
		);
		std::process::exit(1);
		// Self {}
	}
}

impl FirmwareStorage for IntelHexFirmwareFile
{
	fn load_address(&self) -> Option<u32>
	{
		None
	}

	fn firmware_data(&self) -> &[u8]
	{
		&[]
	}
}
