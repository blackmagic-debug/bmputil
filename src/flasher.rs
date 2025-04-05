// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
// SPDX-FileContributor: Modified by Rachel Mant <git@dragonmux.network>

use std::io::{Read, Write};
use std::rc::Rc;

use clap::ArgMatches;
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, warn};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

use crate::bmp::{BmpDevice, FirmwareFormat, FirmwareType};
use crate::elf;
use crate::error::{Error, ErrorKind};

pub struct Firmware
{
    firmware_type: FirmwareType,
    data: Vec<u8>,
    length: u32,
}

impl Firmware
{
    pub fn new(matches: &ArgMatches, device: &BmpDevice, firmware_data: Vec<u8>) -> Result<Self, Error>
    {
        let firmware_length = firmware_data.len();
        let firmware_length = u32::try_from(firmware_length)
            .expect("firmware filesize exceeded 32 bits! Firmware binary must be invalid (too big)");

        Ok(Self {
            firmware_type: Self::determine_firmware_type(matches, device, &firmware_data)?,
            data: firmware_data,
            length: firmware_length,
        })
    }

    fn determine_firmware_type(
        matches: &ArgMatches, device: &BmpDevice, firmware_data: &Vec<u8>
    ) -> Result<FirmwareType, Error>
    {
        // Figure out what kind of firmware we're being asked to work with here
        // Using the platform to determine the link address.
        let platform = device.platform();
        let firmware_type = FirmwareType::detect_from_firmware(platform, &firmware_data)
            .map_err(|e| e.with_ctx("detecting firmware type"))?;

        debug!("Firmware file was detected as {}", firmware_type);

        // But allow the user to override that type, if they *really* know what they are doing.
        let firmware_type = if let Some(location) = matches
            .get_one::<String>("override-firmware-type").map(|s| s.as_str()) {
            if let Some("really") = matches.get_one::<String>("allow-dangerous-options").map(|s| s.as_str()) {
                warn!("Overriding firmware-type detection and flashing to user-specified location ({}) instead!", location);
            } else {
                // We're ignoring errors for setting the color because the most important thing is
                // getting the message itself out.
                // If the messages themselves don't write, though, then we might as well just panic.
                let mut stderr = StandardStream::stderr(ColorChoice::Auto);
                let _res = stderr.set_color(ColorSpec::new().set_fg(Some(Color::Red)));
                write!(&mut stderr, "WARNING: ").expect("failed to write to stderr");
                let _res = stderr.reset();
                writeln!(
                    &mut stderr,
                    "--override-firmware-type is used to override the firmware type detection and flash \
                    a firmware binary to a location other than the one that it seems to be designed for.\n\
                    This is a potentially destructive operation and can result in an unbootable device! \
                    (can require a second, external JTAG debugger and manual wiring to fix!)\n\
                    \nDo not use this option unless you are a firmware developer and really know what you are doing!\n\
                    \nIf you are sure this is really what you want to do, run again with --allow-dangerous-options=really"
                ).expect("failed to write to stderr");
                std::process::exit(1);
            };
            if location == "bootloader" {
                FirmwareType::Bootloader
            } else if location == "application" {
                FirmwareType::Application
            } else {
                unreachable!("Clap ensures invalid option cannot be passed to --override-firmware-type");
            }
        } else {
            firmware_type
        };

        Ok(firmware_type)
    }

    pub fn program_firmware(&self, device: &mut BmpDevice) -> Result<(), Error>
    {
        // We need an Rc<T> as [`dfu_core::sync::DfuSync`] requires `progress` to be 'static,
        // so it must be moved into the closure. However, since we need to call .finish() here,
        // it must be owned by both. Hence: Rc<T>.
        // Default template: `{wide_bar} {pos}/{len}`.
        let progress_bar = ProgressBar::new(self.length as u64)
            .with_style(ProgressStyle::default_bar()
                .template(" {percent:>3}% |{bar:50}| {bytes}/{total_bytes} [{binary_bytes_per_sec} {elapsed}]").unwrap()
            );
        let progress_bar = Rc::new(progress_bar);
        let enclosed = Rc::clone(&progress_bar);
        // Extract the firmware type as a value so it can be captured and moved (copied) by the progress lambda
        let firmware_type = self.firmware_type;

        match device.download(&*self.data, self.length, firmware_type,
            move |flash_pos_delta| {
                // Don't actually print flashing until the erasing has finished.
                if enclosed.position() == 0 {
                    if firmware_type == FirmwareType::Application {
                        enclosed.println("Flashing...");
                    } else {
                        enclosed.println("Flashing bootloader...");
                    }
                }
                enclosed.inc(flash_pos_delta as u64);
        }) {
            Ok(()) => {
                progress_bar.finish();
                Ok(())
            },
            Err(e) => {
                progress_bar.finish();
                if progress_bar.position() == (self.length as u64) {
                    warn!("Possibly spurious error from OS at the very end of flashing: {}", e);
                    Ok(())
                } else {
                    Err(e)
                }
            },
        }
    }
}

fn intel_hex_error() -> !
{
    // We're ignoring errors for setting the color because the most important thing
    // is getting the message itself out.
    // If the messages themselves don't write, though, then we might as well just panic.
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);
    let _res = stderr.set_color(ColorSpec::new().set_fg(Some(Color::Red)));
    write!(&mut stderr, "Error: ")
        .expect("failed to write to stderr");
    let _res = stderr.reset();
    writeln!(
        &mut stderr,
        "The specified firmware file appears to be an Intel HEX file, but Intel HEX files are not \
        currently supported. Please use a binary file (e.g. blackmagic.bin), \
        or an ELF (e.g. blackmagic.elf) to flash.",
    )
    .expect("failed to write to stderr");

    std::process::exit(1);
}

pub fn read_firmware(file_name: &str) -> Result<Vec<u8>, Error>
{
    let firmware_file = std::fs::File::open(file_name)
        .map_err(|source| ErrorKind::FirmwareFileIo(Some(file_name.to_string())).error_from(source))
        .map_err(|e| e.with_ctx("reading firmware file to flash"))?;

    let mut firmware_file = std::io::BufReader::new(firmware_file);

    let mut firmware_data = Vec::new();
    firmware_file.read_to_end(&mut firmware_data).unwrap();

    // FirmwareFormat::detect_from_firmware() needs at least 4 bytes, and
    // FirmwareType::detect_from_firmware() needs at least 8 bytes,
    // but also if we don't even have 8 bytes there's _no way_ this is valid firmware.
    if firmware_data.len() < 8 {
        return Err(
            ErrorKind::InvalidFirmware(Some("less than 8 bytes long".into())).error()
        );
    }

    // Extract the actual firmware data from the file, based on the format we're using.
    let firmware_data = match FirmwareFormat::detect_from_firmware(&firmware_data) {
        FirmwareFormat::Binary => firmware_data,
        FirmwareFormat::Elf => elf::extract_binary(&firmware_data)?,
        FirmwareFormat::IntelHex => intel_hex_error(), // FIXME: implement this.
    };

    Ok(firmware_data)
}
