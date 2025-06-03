// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
// SPDX-FileContributor: Written by Piotr Esden-Tempski <piotr@esden.net>
// SPDX-FileContributor: Modified by Rachel Mant <git@dragonmux.network>

use crate::bmp::FirmwareType;

pub mod bmp;
pub mod docs_viewer;
pub mod error;
pub mod elf;
pub mod firmware_selector;
pub mod flasher;
pub mod metadata;
pub mod switcher;
pub mod usb;
#[cfg(windows)]
pub mod windows;
pub mod probe_identity;

pub trait BmpParams
{
    fn index(&self) -> Option<usize>;
    fn serial_number(&self) -> Option<&str>;
    fn allow_dangerous_options(&self) -> AllowDangerous;
}

pub trait FlashParams
{
    fn override_firmware_type(&self) -> Option<FirmwareType>;
}

#[derive(Clone, Copy)]
pub enum AllowDangerous
{
    Never,
    Really
}
