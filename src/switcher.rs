// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::fs;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use color_eyre::eyre::{eyre, Result};
use dialoguer::theme::ColorfulTheme;
use dialoguer::Select;
use directories::ProjectDirs;
use indicatif::ProgressBar;

use crate::BmpParams;
use crate::bmp::BmpDevice;
use crate::bmp::BmpMatcher;
use crate::firmware_selector::FirmwareMultichoice;
use crate::flasher;
use crate::metadata::download_metadata;
use crate::metadata::structs::{Firmware, FirmwareDownload, Metadata};
use crate::probe_identity::{ProbeIdentity, Version};
use crate::FlashParams;

pub fn switch_firmware<Params>(params: &Params, paths: &ProjectDirs) -> Result<()>
where
    Params: BmpParams + FlashParams,
{
    // Start by figuring out which probe to use for the operation
    let probe = match select_probe(params)? {
        Some(probe) => probe,
        None => {
            println!("Black Magic Debug probe selection cancelled, stopping operation");
            return Ok(());
        }
    };

    let firmware_identity_string = probe.firmware_identity()?;
    println!("Probe {} ({}) selected for firmware update", firmware_identity_string, probe.serial_number()?);

    // Now extract the probe's identification, and check it's valid
    let identity: ProbeIdentity = firmware_identity_string.try_into().map_err(|err| {
        println!("Couldn't extract an identity from firmware descriptor string: {}", err);
        eyre!("Couldn't extract an identity from firmware descriptor string")
    })?;

    // Figure out where the firmware and metadata cache is
    let cache = paths.cache_dir();

    // Grab down the metadata index
    let metadata = download_metadata(cache)?;
    let (release, firmware) = match pick_release(&metadata, identity)? {
        Some(firmware) => firmware,
        None => {
            println!("firmware release selection cancelled, stopping operation");
            return Ok(())
        }
    };

    // Now see which variant of the firmware the user wants to use
    let firmware_variant = match pick_firmware(release, firmware)? {
        Some(variant) => variant,
        None => {
            println!("firmware variant selection cancelled, stopping operation");
            return Ok(())
        }
    };

    // Download the firmware (or extract it from the cache)
    let elf_file = download_firmware(firmware_variant, cache)?;

    // Having done all of that, finally try to Flash the new firmware on the probe
    flasher::flash_probe(params, probe, elf_file)
}

fn select_probe<Params>(params: &Params) -> Result<Option<BmpDevice>>
where
    Params: BmpParams,
{
    // Start by seeing if there are any probes, filtered by any match critera supplied
    let matcher = BmpMatcher::from_params(params);
    let mut results = matcher.find_matching_probes();
    // Turn that into a list of devices (if there were no devices found, this turns
    // that into an error appropriately)
    let mut devices = results.pop_all()?;
    // Figure out what to do based on the numeber of matching probes
    match devices.len() {
        0 => panic!("This state shouldn't happen, because devices should contain at least 1"),
        // If we have just one probe, return that and be done
        1 => Ok(Some(devices.remove(0))),
        // Otherwise, we've got more than one, so ask the user to make a choice
        _ => {
            // Map the device list to create selection items
            let items: Vec<_> = devices
                .iter()
                .flat_map(
                    |device| -> Result<String> {
                        Ok(format!("{} ({})", device.firmware_identity()?, device.serial_number()?))
                    }
                )
                .collect();

            // Figure out which one the user wishes to use
            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Which probe would you like to change the firmware on?")
                .items(items.as_slice())
                .report(false)
                .interact_opt()?;
            // Extract and return that one, if the user didn't cancel selection
            Ok(selection.map(|index| devices.remove(index)))
        },
    }
}

fn pick_release(metadata: &Metadata, identity: ProbeIdentity) -> Result<Option<(&str, &Firmware)>>
{
    let variant = &identity.variant();
    let firmware_version = match &identity.version {
        // If we don't know what version of firmware is on the probe, presume it's v1.6 for now.
        // We can't actually know which prior version to v1.6 it actually is, but it's very old either way
        Version::Unknown => {
            println!("Old firmware version is detected, pretending this is version 'v1.6'");
            "v1.6"
        },
        Version::Known(firmware) => firmware
    };

    // Filter out releases that don't support this probe, and filter out the one the probe is currently running
    // if there is only a single variant in the release (multi-variant releases still need to be shown)
    let releases: Vec<_> = metadata.releases
        .iter()
        .filter(
            |&(version, release)|
                !(firmware_version == version && release.firmware[variant].variants.len() == 1) &&
                    release.firmware.contains_key(&variant)
        )
        .collect();

    let mut items: Vec<_> = releases
        .iter()
        .map(|&(version, _)| version)
        .collect();
    items.sort_by(|a, b| a.cmp(b).reverse());

    // Ask the user to choose a release, sorting the releases newest-to-oldest
    let selection = match Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Which release would you like to run on your probe?")
        .items(items.as_slice())
        .interact_opt()? {
        Some(release) => release,
        None => return Ok(None),
    };
    Ok(Some((items[selection].as_str(), &metadata.releases[items[selection].as_str()].firmware[&variant])))
}

fn pick_firmware<'a>(release: &'a str, firmware: &'a Firmware) -> Result<Option<&'a FirmwareDownload>>
{
    match firmware.variants.len() {
        // If there are now firmware variants for this release, that's an error
        0 => Ok(None),
        // If there is only one variant, then that's what the user gets.. display and return
        1 => {
            let (_, firmware) = firmware.variants.iter().nth(0).unwrap();
            println!("Using firmware {}", firmware.friendly_name);
            Ok(Some(firmware))
        }
        // Otherwise, if there's more than one we have to ask the user to make a choice
        _ => {
            // Enter the selection FSM to either extract a selection from the user, or cancellation
            let mut chooser = FirmwareMultichoice::new(release, &firmware.variants);
            while !chooser.complete() {
                chooser.step()?;
            }
            Ok(chooser.selection())
        }
    }
}

fn download_firmware(variant: &FirmwareDownload, cache_path: &Path) -> Result<PathBuf>
{
    // Ensure the cache directory exists
    fs::create_dir_all(cache_path)?;
    // Extract the local name for the firmware file
    let file_name = &variant.file_name;
    // If the file exists in the cache already, then return that path
    let cache_file_name = cache_path.join(file_name);
    if cache_file_name.exists() {
        return Ok(cache_file_name);
    }

    // Set up a progress ticker so the user knows something is happening
    let progress = ProgressBar::new_spinner()
        .with_message("Downloading requested firmware");
    // Tick the spinner once every 100ms so we get a smooth showing of progress
    progress.enable_steady_tick(Duration::from_millis(100));

    // Otherwise, we don't yet have this firmware cached, so let's download it!
    let client = reqwest::blocking::Client::new();
    let mut response = client.get(variant.uri.clone())
        // Use a 2 second timeout so we don't get stuck forever if the user is
        // having connectivity problems - better to die early and have them retry
        .timeout(Duration::from_secs(2))
        .send()?
        .error_for_status()?;

    // Write the downloaded ELF out to its cache file
    let mut cache_file = File::create(cache_file_name.as_path())?;
    response.copy_to(&mut cache_file)?;

    // Finish that progress spinner so the user sees the download finished
    progress.finish();

    // Return where that is for further use
    Ok(cache_file_name)
}
