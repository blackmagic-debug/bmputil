// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::iter::zip;
use std::ops::Range;
use std::{collections::BTreeMap, fs::File, io::Read};

use color_eyre::eyre::{Report, Result, eyre};
use goblin::container::Endian;
use goblin::elf::program_header::PT_LOAD;
use goblin::elf::{Elf, header::{EI_CLASS, ELFCLASS32, EM_ARM, ET_EXEC}};
use log::debug;

use super::FirmwareStorage;

pub struct ELFFirmwareFile
{
	contents: Box<[u8]>,
	segments: BTreeMap<u32, Range<usize>>,
	firmware_image: Box<[u8]>,
}

impl TryFrom<File> for ELFFirmwareFile
{
	type Error = Report;

	fn try_from(mut file: File) -> Result<Self>
	{
		debug!("Loading file as ELF firmware binary");
		// Extract the contents of the ELF file into memory
		let mut contents = Vec::new();
		file.read_to_end(&mut contents)?;
		let contents = contents.into_boxed_slice();

		// Try to parse the file as an ELF
		let elf = Elf::parse(&contents)?;

		// Validate the header is for a 32-bit ARM device
		let header = elf.header;
		if header.e_type != ET_EXEC || header.e_machine != EM_ARM ||
			header.endianness()? != Endian::Little || header.e_ident[EI_CLASS] != ELFCLASS32 {
			return Err(eyre!("ELF does not represent firmware for a Black Magic Debug device"));
		}

		// Extract loadable non-zero-length program headers
		let segments = elf.program_headers
			.iter()
			.flat_map(|header| {
				// Map into base address + file byte range for the data covered by the segment
				(header.p_type == PT_LOAD && header.p_filesz != 0)
					.then_some((header.p_paddr as u32, header.file_range()))
			})
			.collect::<BTreeMap<_, _>>();
		debug!("Consuming {} segments from file", segments.len());

		// Make one of ourself
		let mut result = Self {
			contents,
			segments,
			firmware_image: Box::default(),
		};
		// Use the data to make the firmware image
		result.build_firmware_image();

		Ok(result)
	}
}

impl ELFFirmwareFile
{
	fn build_firmware_image(&mut self)
	{
		// Figure out where the first segment sits
		let load_address = self.load_address().unwrap_or(0);
		debug!("Firmware image loads at 0x{load_address:08x}");

		// Extract slices for each of the segments ready to flatten out
		let segments_data = self.segments
			.values()
			.map(|range| {
				&self.contents[range.clone()]
			})
			.collect::<Vec<_>>();

		// Extract a set of position and length ranges
		let segment_ranges = self.segments
			.iter()
			.map(|(&address, range)| {
				address..address + (range.len() as u32)
			})
			.collect::<Vec<_>>();

		// Figure out the total length of the flattened firmware image to allocate
		let total_length = segment_ranges
			.clone()
			.into_iter()
			.reduce(|a, b| {
				a.start..b.end
			})
			.map(|range| range.len())
			.unwrap_or(0);
		debug!("Firmware is {total_length} bytes long flattened");

		// Allocate enough memory to hold the completely flattened firmware image
		// and initialise it to the erased byte value
		let mut firmware_image = vec![0xffu8; total_length].into_boxed_slice();

		// Loop through all the segments of data and copy them to their final resting places
		// in the flattened image
		for (segment, position) in zip(segments_data, segment_ranges) {
			// Calculate the location of the segment in the flattened image
			let range = ((position.start - load_address) as usize)..((position.end - load_address) as usize);
			// Grab the relevant slice and copy the segment data in
			firmware_image[range].copy_from_slice(segment);
		}

		// Store the resulting image back on ourself
		self.firmware_image = firmware_image;
	}
}

impl FirmwareStorage for ELFFirmwareFile
{
	fn load_address(&self) -> Option<u32>
	{
		// Extract the first segment (the map ordered them for us) and return its address
		self.segments.first_key_value().map(|(&address, _)| address)
	}

	fn firmware_data(&self) -> &[u8]
	{
		&self.firmware_image
	}
}
