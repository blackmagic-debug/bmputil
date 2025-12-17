// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::fs::File;
use std::io::Read;

use color_eyre::eyre::{Report, Result, eyre};
use owo_colors::OwoColorize;

use super::FirmwareStorage;

pub struct IntelHexFirmwareFile {}

struct IntelHexRecord
{
	byte_count: u8,
	address: u16,
	record_type: IntelHexRecordType,
	data: [u8; 255],
}

#[repr(u8)]
#[non_exhaustive]
enum IntelHexRecordType
{
	Data = 0x00,
	EndOfFile = 0x01,
	ExtendedSegmentAddress = 0x02,
	StartSegmentAddress = 0x03,
	ExtendedLinearAddress = 0x04,
	StartLinearAddress = 0x05,
}

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

impl IntelHexRecord
{
	pub fn new(file: &mut File) -> Result<Self>
	{
		let mut data = [0];
		// Consume bytes from `file` till we find an opening `:`
		loop {
			// This'll explode with an unexpected EOF error if we can't find a `:`
			file.read_exact(&mut data)?;
			if data[0] == b':' {
				break;
			}
		}

		// Set up the line checksum
		let mut actual_checksum = 0u8;

		// Read 2 bytes to interpret as the byte count and convert from ASCII hex
		let mut data = [0; 2];
		file.read_exact(&mut data)?;
		let byte_count = u8::from_str_radix(str::from_utf8(&data)?, 16)?;
		actual_checksum += byte_count;

		// Read 4 bytes to interpret as an address
		let mut data = [0; 4];
		file.read_exact(&mut data)?;
		let address = u16::from_str_radix(str::from_utf8(&data)?, 16)?;
		actual_checksum += ((address >> 8) as u8) + (address as u8);

		// Read 2 bytes to interpret as the record type
		let mut data = [0; 2];
		file.read_exact(&mut data)?;
		let record_type = u8::from_str_radix(str::from_utf8(&data)?, 16)?;
		actual_checksum += record_type;

		// Read byte_count byte pairs into a buffer sized to take it
		let len = byte_count as usize;
		let mut bytes = vec![0; len * 2];
		file.read_exact(&mut bytes[0..(len * 2)])?;
		// De-hexify the bytes
		for idx in 0..len {
			let begin = idx * 2;
			let end = begin + 2;
			bytes[idx] =  u8::from_str_radix(str::from_utf8(&bytes[begin..end])?, 16)?;
			actual_checksum += bytes[idx];
		}

		// Read 2 bytes to interpret as the checksum
		let mut data = [0; 2];
		file.read_exact(&mut data)?;
		let expected_checksum = u8::from_str_radix(str::from_utf8(&data)?, 16)?;
		if expected_checksum != !actual_checksum {
			return Err(eyre!("Checksum invalid for ihex record"));
		}

		// Make a buffer to hold the decoded data in and copy the finalised data into it
		let mut data = [0xff; 255];
		data[0..len].copy_from_slice(&bytes[0..len]);

		Ok(Self {
			byte_count,
			address,
			record_type: IntelHexRecordType::try_from(record_type)?,
			data,
		})
	}
}

impl TryFrom<u8> for IntelHexRecordType
{
	type Error = Report;

	fn try_from(value: u8) -> Result<Self>
	{
		match value {
			0 => Ok(Self::Data),
			1 => Ok(Self::EndOfFile),
			2 => Ok(Self::ExtendedSegmentAddress),
			3 => Ok(Self::StartSegmentAddress),
			4 => Ok(Self::ExtendedLinearAddress),
			5 => Ok(Self::StartLinearAddress),
			_ => Err(eyre!("Invalid record type {value} found")),
		}
	}
}
