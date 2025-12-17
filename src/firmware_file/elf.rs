// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::ops::Range;
use std::{collections::BTreeMap, fs::File, io::Read};

use color_eyre::eyre::{Report, Result, eyre};
use goblin::container::Endian;
use goblin::elf::program_header::PT_LOAD;
use goblin::elf::{Elf, header::{EI_CLASS, ELFCLASS32, EM_ARM, ET_EXEC}};

use super::FirmwareStorage;

pub struct ELFFirmwareFile
{
	contents: Box<[u8]>,
	segments: BTreeMap<u32, Range<usize>>
}

impl TryFrom<File> for ELFFirmwareFile
{
	type Error = Report;

	fn try_from(mut file: File) -> Result<Self>
	{
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
			.flat_map(|header|
			{
				// Map into base address + file byte range for the data covered by the segment
				(header.p_type == PT_LOAD && header.p_filesz != 0)
					.then_some((header.p_paddr as u32, header.file_range()))
			})
			.collect::<BTreeMap<_, _>>();

		Ok(Self {
			contents,
			segments,
		})
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
