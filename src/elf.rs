use goblin::elf::Elf;

trait ElfExt
{
    /// Get a reference to a section header with the given name.
    fn get_section_by_name(&self, name: &str) -> Option<&goblin::elf::SectionHeader>;
}

impl<'a> ElfExt for Elf<'a>
{
    fn get_section_by_name(&self, name: &str) -> Option<&goblin::elf::SectionHeader>
    {
        for section in &self.section_headers {
            let parsed_name = self.shdr_strtab.get_at(section.sh_name).unwrap();

            if parsed_name == name {
                return Some(section);
            }
        }

        None
    }
}

trait SectionHeaderExt
{
    /// Get the raw data of this section, given the full ELF data.
    fn get_data<'s>(&'s self, parent_data: &'s [u8]) -> &'s [u8];
}

impl SectionHeaderExt for goblin::elf::SectionHeader
{
    fn get_data<'s>(&'s self, parent_data: &'s [u8]) -> &'s [u8]
    {
        let start_idx = self.sh_offset as usize;
        let size = self.sh_size;
        let end_idx = start_idx + size as usize;
        let data: &[u8] = &parent_data[start_idx..end_idx];

        data
    }
}


pub fn extract(elf_data: &[u8]) -> Vec<u8>
{
    let elf = Elf::parse(elf_data).unwrap();

    // FIXME: Dynamically detect what sections should be copied.
    // arm-none-eabi-objcopy seems to only copy these three, but I'm not yet certain why only these three
    // (as these aren't the only three that have PROGBITS set).

    let text = elf
        .get_section_by_name(".text")
        .unwrap()
        .get_data(elf_data);

    let arm_exidx = elf
        .get_section_by_name(".ARM.exidx")
        .unwrap()
        .get_data(elf_data);

    let data = elf
        .get_section_by_name(".data")
        .unwrap()
        .get_data(elf_data);

    let mut extracted = Vec::with_capacity(text.len() + arm_exidx.len() + data.len());

    extracted.extend_from_slice(text);
    extracted.extend_from_slice(arm_exidx);
    extracted.extend_from_slice(data);

    extracted
}
