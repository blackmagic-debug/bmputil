// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::fs::File;
use std::path::Path;

use color_eyre::eyre::Result;

pub struct GdbRspInterface
{
	#[allow(unused)]
	interface: File,
}

impl GdbRspInterface
{
	pub fn from_path(serial_port: &Path) -> Result<Self>
	{
		Ok(Self {
			interface: File::options().read(true).write(true).open(serial_port)?,
		})
	}
}
