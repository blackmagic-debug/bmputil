// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

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
