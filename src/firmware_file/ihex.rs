// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{ErrorKind, Read, Seek};

use color_eyre::eyre::{Report, Result, eyre};
use log::debug;

use super::FirmwareStorage;

pub struct IntelHexFirmwareFile
{
	segments: BTreeMap<u32, Box<[u8]>>,
	firmware_image: Box<[u8]>,
}

struct IntelHexRecord
{
	pub byte_count: u8,
	pub address: u16,
	pub record_type: IntelHexRecordType,
	pub data: [u8; 255],
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

struct IntelHexSegment
{
	base_address: u32,
	data: Box<[u8]>,
}

impl TryFrom<File> for IntelHexFirmwareFile
{
	type Error = Report;

	fn try_from(mut file: File) -> Result<Self>
	{
		debug!("Loading file as Intel HEX firmware binary");

		// Set up a vec to receive the records, and a buffer to receive newline characters
		let mut records = Vec::new();
		let mut buf = [0];
		let mut eof = false;
		while !eof {
			// Try to read a record and stuff it into the records vec
			records.push(IntelHexRecord::try_from(&mut file)?);
			// Read characters and test if they're new lines
			while !eof {
				match file.read(&mut buf) {
					Ok(0) => eof = true,
					Ok(_) => {
						// `buf` now contains a valid character.. see what it was
						if !matches!(buf[0], b'\r' | b'\n') {
							file.seek_relative(-1)?;
							break;
						}
					},
					Err(ref error) if error.kind() == ErrorKind::Interrupted => {},
					Err(error) => return Err(error.into()),
				}
			}
		}
		debug!("Read {} records", records.len());

		// Process all the records from the file into segment data
		let segments = IntelHexSegment::from_records(&records)?
			.into_iter()
			// Remap the segments to turn them into a BTreeMap so we get them all in address order
			.map(|segment| (segment.base_address, segment.data))
			.collect();

		// Make one of ourself with the segments data we've now collected
		let mut result = Self {
			segments,
			firmware_image: Box::default(),
		};
		// Use the data to make the firmware image
		result.build_firmware_image();

		Ok(result)
	}
}

impl IntelHexFirmwareFile
{
	fn build_firmware_image(&mut self)
	{
		// Figure out where the first segment sits
		let load_address = self.load_address().unwrap_or(0);
		debug!("Firmware image loads at 0x{load_address:08x}");

		// Figure out the total length of the flattened firmware image to allocate
		let total_length = self
			.segments
			.iter()
			.map(|(&base_address, segment)| base_address..base_address + segment.len() as u32)
			.reduce(|a, b| a.start..b.end)
			.map(|range| range.len())
			.unwrap_or(0);
		debug!("Firmware is {total_length} bytes long flattened");

		// Allocate enough memory to hold the completely flattened firmware image
		// and initialise it to the erased byte value
		let mut firmware_image = vec![0xffu8; total_length].into_boxed_slice();

		// Loop through all the segments of data and copy them to their final resting places
		// in the flattened image
		for (base_address, segment) in &self.segments {
			// Calculate the location of the segment in the flattened image
			let begin = (base_address - load_address) as usize;
			let range = begin..begin + segment.len();
			// Grab the relevant slice and copy the segment data in
			firmware_image[range].copy_from_slice(segment);
		}

		// Store the resulting image back on ourself
		self.firmware_image = firmware_image;
	}
}

impl IntelHexSegment
{
	pub fn from_records(records: &[IntelHexRecord]) -> Result<Box<[Self]>>
	{
		let mut base_address = 0;
		let mut begin_address = 0;
		let mut end_address = 0;
		let mut segment_data = Vec::new();
		let mut segments = Vec::new();

		for (idx, record) in records.iter().enumerate() {
			match record.record_type {
				IntelHexRecordType::Data => {
					// Compute the block's address, and make its length usize
					let address = base_address + record.address as u32;
					let length = record.byte_count as usize;
					// If this is not contiguous with the last end address, build a segment and
					// set things up to take more data
					if end_address != address {
						if end_address != begin_address {
							segments.push(Self {
								base_address: begin_address,
								data: segment_data.into_boxed_slice(),
							});
						}
						begin_address = address;
						end_address = address;
						segment_data = Vec::new();
					}
					// Add the data from this record to the segment data
					segment_data.extend_from_slice(&record.data[0..length]);
					end_address += length as u32;
				},
				IntelHexRecordType::EndOfFile => {
					// Check that the EOF record is actually the last one
					if idx + 1 != records.len() {
						return Err(eyre!("Premature EOF record found, invalid Intel HEX file"));
					}
					// Take any remaining segment data and shove that into the segments vec
					if end_address != begin_address {
						segments.push(Self {
							base_address: begin_address,
							data: segment_data.into_boxed_slice(),
						});
						segment_data = Vec::new();
					}
				},
				IntelHexRecordType::ExtendedLinearAddress => {
					let bytes = record.data[0..2].try_into()?;
					let address_high = u16::from_be_bytes(bytes);
					base_address = (address_high as u32) << 16;
				},
				IntelHexRecordType::StartLinearAddress => {
					let bytes = record.data[0..4].try_into()?;
					base_address = u32::from_be_bytes(bytes);
				},
				_ => todo!(),
			}
		}

		debug!("Recovered {} segments from data stream", segments.len());
		Ok(segments.into_boxed_slice())
	}
}

impl FirmwareStorage for IntelHexFirmwareFile
{
	fn load_address(&self) -> Option<u32>
	{
		self.segments.first_key_value().map(|(&address, _)| address)
	}

	fn firmware_data(&self) -> &[u8]
	{
		&self.firmware_image
	}
}

impl TryFrom<&mut File> for IntelHexRecord
{
	type Error = Report;

	fn try_from(file: &mut File) -> Result<Self>
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
		actual_checksum = actual_checksum.wrapping_add(byte_count);

		// Read 4 bytes to interpret as an address
		let mut data = [0; 4];
		file.read_exact(&mut data)?;
		let address = u16::from_str_radix(str::from_utf8(&data)?, 16)?;
		actual_checksum = actual_checksum
			.wrapping_add((address >> 8) as u8)
			.wrapping_add(address as u8);

		// Read 2 bytes to interpret as the record type
		let mut data = [0; 2];
		file.read_exact(&mut data)?;
		let record_type = u8::from_str_radix(str::from_utf8(&data)?, 16)?;
		actual_checksum = actual_checksum.wrapping_add(record_type);

		// Read byte_count byte pairs into a buffer sized to take it
		let len = byte_count as usize;
		let mut bytes = vec![0; len * 2];
		file.read_exact(&mut bytes[0..(len * 2)])?;
		// De-hexify the bytes
		for idx in 0..len {
			let begin = idx * 2;
			let end = begin + 2;
			bytes[idx] = u8::from_str_radix(str::from_utf8(&bytes[begin..end])?, 16)?;
			actual_checksum = actual_checksum.wrapping_add(bytes[idx]);
		}

		// Read 2 bytes to interpret as the checksum
		let mut data = [0; 2];
		file.read_exact(&mut data)?;
		let expected_checksum = u8::from_str_radix(str::from_utf8(&data)?, 16)?;
		// Two's complement the checksum to check it
		if expected_checksum != (!actual_checksum).wrapping_add(1) {
			return Err(eyre!("Checksum invalid for ihex record"));
		}

		// Make a buffer to hold the decoded data in and copy the finalised data into it
		let mut data = [0xff; 255];
		data[0..len].copy_from_slice(&bytes[0..len]);

		// Convert the record type and do any record-specific validation
		let record_type = IntelHexRecordType::try_from(record_type)?;
		record_type.validate_byte_count(byte_count)?;

		Ok(Self {
			byte_count,
			address,
			record_type,
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

impl IntelHexRecordType
{
	pub fn validate_byte_count(&self, byte_count: u8) -> Result<()>
	{
		match self {
			Self::EndOfFile => {
				if byte_count == 0 {
					Ok(())
				} else {
					Err(eyre!("Invalid EOF, expected 0 bytes in record, got {byte_count}"))
				}
			},
			Self::ExtendedSegmentAddress | Self::ExtendedLinearAddress => {
				if byte_count == 2 {
					Ok(())
				} else {
					Err(eyre!("Invalid extended address record, expected 2 bytes, got {byte_count}"))
				}
			},
			Self::StartSegmentAddress | Self::StartLinearAddress => {
				if byte_count == 4 {
					Ok(())
				} else {
					Err(eyre!("Invalid extended address record, expected 4 bytes, got {byte_count}"))
				}
			},
			_ => Ok(()),
		}
	}
}
