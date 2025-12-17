// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
// SPDX-FileContributor: Written by Piotr Esden-Tempski <piotr@esden.net>
// SPDX-FileContributor: Modified by Rachel Mant <git@dragonmux.network>

use clap::ValueEnum;
use clap::builder::PossibleValue;

use crate::bmp::FirmwareType;

pub mod bmp;
mod bmp_matcher;
pub mod docs_viewer;
pub mod error;
pub mod firmware_file;
pub mod firmware_selector;
pub mod flasher;
pub mod metadata;
pub mod probe_identity;
pub mod serial;
pub mod switcher;
pub mod usb;
#[cfg(windows)]
pub mod windows;

pub trait BmpParams
{
	fn index(&self) -> Option<usize>;
	fn serial_number(&self) -> Option<&str>;
}

pub trait FlashParams
{
	fn allow_dangerous_options(&self) -> AllowDangerous;
	fn override_firmware_type(&self) -> Option<FirmwareType>;
}

#[derive(Clone, Copy)]
pub enum AllowDangerous
{
	Never,
	Really,
}

impl ValueEnum for AllowDangerous
{
	fn value_variants<'a>() -> &'a [Self]
	{
		&[Self::Never, Self::Really]
	}

	fn to_possible_value(&self) -> Option<PossibleValue>
	{
		match self {
			Self::Never => Some("never".into()),
			Self::Really => Some("really".into()),
		}
	}
}
