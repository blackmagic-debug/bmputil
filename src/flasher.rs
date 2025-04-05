// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
// SPDX-FileContributor: Modified by Rachel Mant <git@dragonmux.network>

use std::rc::Rc;

use indicatif::{ProgressBar, ProgressStyle};
use log::warn;

use crate::bmp::{BmpDevice, FirmwareType};
use crate::error::Error;

pub fn program_firmware(
    device: &mut BmpDevice, firmware_type: FirmwareType, firmware_data: Vec<u8>, firmware_length: u32
) -> Result<(), Error>
{
    // We need an Rc<T> as [`dfu_core::sync::DfuSync`] requires `progress` to be 'static,
    // so it must be moved into the closure. However, since we need to call .finish() here,
    // it must be owned by both. Hence: Rc<T>.
    // Default template: `{wide_bar} {pos}/{len}`.
    let progress_bar = ProgressBar::new(firmware_length as u64)
        .with_style(ProgressStyle::default_bar()
            .template(" {percent:>3}% |{bar:50}| {bytes}/{total_bytes} [{binary_bytes_per_sec} {elapsed}]").unwrap()
        );
    let progress_bar = Rc::new(progress_bar);
    let enclosed = Rc::clone(&progress_bar);

    match device.download(&*firmware_data, firmware_length, firmware_type, move |flash_pos_delta| {
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
            if progress_bar.position() == (firmware_length as u64) {
                warn!("Possibly spurious error from OS at the very end of flashing: {}", e);
                Ok(())
            } else {
                Err(e)
            }
        },
    }
}
