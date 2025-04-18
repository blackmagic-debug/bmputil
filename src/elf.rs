// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
// SPDX-FileContributor: Modified by Rachel Mant <git@dragonmux.network>

use color_eyre::eyre::Result;
use goblin::elf::{Elf, SectionHeader};
use goblin::error::Error as GoblinError;

/// Convenience extensions to [Elf].
trait ElfExt
{
    /// Get a reference to a section header with the given name. Returns None if
    /// a section by that name does not exist.
    fn get_section_by_name(&self, name: &str) -> Option<&goblin::elf::SectionHeader>;
}

impl<'a> ElfExt for Elf<'a>
{
    fn get_section_by_name(&self, name: &str) -> Option<&goblin::elf::SectionHeader>
    {
        for section in &self.section_headers {
            let parsed_name = self.shdr_strtab.get_at(section.sh_name)?;

            if parsed_name == name {
                return Some(section);
            }
        }

        None
    }
}

/// Convenience extensions to [SectionHeader].
trait SectionHeaderExt
{
    /// Get the raw data of this section, given the full ELF data.
    fn get_data<'s>(&'s self, parent_data: &'s [u8]) -> Result<&'s [u8]>;
}

impl SectionHeaderExt for SectionHeader
{
    fn get_data<'s>(&'s self, parent_data: &'s [u8]) -> Result<&'s [u8]>
    {
        let start_idx = self.sh_offset as usize;
        let size = self.sh_size;
        let end_idx = start_idx + size as usize;
        let data: &[u8] = parent_data.get(start_idx..end_idx)
            .ok_or_else(|| GoblinError::Malformed(format!(
                "ELF section header does not point to a valid section (offset [{}..{}])",
                start_idx,
                end_idx,
            )))?;

        Ok(data)
    }
}


/// Extracts binary data from raw ELF data.
///
/// This should be equivalent to `$ arm-none-eabi-objcopy -Obinary`, but is not yet robust
/// enough to automatically detect what sections should be copied.
/// Currently, `.text`, `.ARM.exidx`, and `.data` are copied.
pub fn extract_binary(elf_data: &[u8]) -> Result<Vec<u8>>
{
    let elf = Elf::parse(elf_data)?;

    // FIXME: Dynamically detect what sections should be copied.
    // arm-none-eabi-objcopy seems to only copy these three, but I'm not yet certain why only these three
    // (as these aren't the only three that have PROGBITS set).

    let text = elf
        .get_section_by_name(".text")
        .ok_or_else(|| GoblinError::Malformed("ELF .text section not found".into()))?
        .get_data(elf_data)?;

    // Allow .ARM.exidx to not exist.
    let arm_exidx = elf
        .get_section_by_name(".ARM.exidx")
        .map(|v| v.get_data(elf_data).ok())
        .flatten();
    let arm_exidx_len = arm_exidx.map(|sect| sect.len()).unwrap_or(0);

    let data = elf
        .get_section_by_name(".data")
        .ok_or_else(|| GoblinError::Malformed("ELF .data section not found".into()))?
        .get_data(elf_data)?;


    let mut extracted = Vec::with_capacity(text.len() + arm_exidx_len + data.len());

    extracted.extend_from_slice(text);
    if let Some(arm_exidx) = arm_exidx {
        extracted.extend_from_slice(arm_exidx);
    }

    extracted.extend_from_slice(data);

    Ok(extracted)
}
