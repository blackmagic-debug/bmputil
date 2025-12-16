// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::fs::File;

use super::FirmwareStorage;

pub struct ELFFirmwareFile {}

impl From<File> for ELFFirmwareFile
{
	fn from(_file: File) -> Self
	{
		Self {}
	}
}

impl FirmwareStorage for ELFFirmwareFile
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
