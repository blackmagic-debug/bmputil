// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use clap::ArgMatches;
use dialoguer::theme::ColorfulTheme;
use dialoguer::Select;
use directories::ProjectDirs;
use log::error;

use crate::bmp::BmpDevice;
use crate::bmp::BmpMatcher;
use crate::error::Error;
use crate::error::ErrorKind;
use crate::metadata::download_metadata;
use crate::metadata::structs::Firmware;
use crate::metadata::structs::FirmwareDownload;
use crate::metadata::structs::Metadata;
use crate::metadata::structs::Probe;

const BMP_PRODUCT_STRING: &str = "Black Magic Probe";
const BMP_PRODUCT_STRING_LENGTH: usize = BMP_PRODUCT_STRING.len();

struct ProbeIdentity
{
    probe: Option<String>,
    pub version: Option<String>,
}

pub fn switch_firmware(matches: &ArgMatches, paths: &ProjectDirs) -> Result<(), Error>
{
    // Start by figuring out which probe to use for the operation
    let probe = match select_probe(matches)? {
        Some(probe) => probe,
        None => {
            println!("Black Magic Debug probe selection cancelled, stopping operation");
            return Ok(());
        }
    };
    println!("Probe {} ({}) selected for firmware update", probe.firmware_identity()?, probe.serial_number()?);

    // Now extract the probe's identification, and check it's valid
    let identity = parse_firmware_identity(&probe.firmware_identity()?);
    let variant = identity.variant()?;
    // If we don't know what version of firmware is on the probe, presume it's v1.6 for now..
    // We can't actually know which prior version to v1.6 it actually is but it's very old either way
    let firmware_version = identity.version.unwrap_or_else(|| "v1.6".into());

    // Grab down the metadata index
    let metadata = download_metadata()?;
    let firmware = match pick_release(&metadata, &variant, &firmware_version)? {
        Some(firmware) => firmware,
        None => {
            println!("firmware release selection cancelled, stopping operation");
            return Ok(())
        }
    };

    // Now see which variant of the firmware the user wants to use
    let firmware_variant = match pick_firmware(firmware)? {
        Some(variant) => variant,
        None => {
            println!("firmware variant selection cancelled, stopping operation");
            return Ok(())
        }
    };

    // Figure out where the firmware cache is
    let cache = paths.cache_dir();

    Ok(())
}

fn select_probe(matches: &ArgMatches) -> Result<Option<BmpDevice>, Error>
{
    // Start by seeing if there are any probes, filtered by any match critera supplied
    let matcher = BmpMatcher::from_cli_args(matches);
    let mut results = matcher.find_matching_probes();
    // Turn that into a list of devices (if there were no devices found, this turns
    // that into an error appropriately)
    let mut devices = results.pop_all()?;
    // Figure out what to do based on the numeber of matching probes
    match devices.len() {
        // If we have just one probe, return that and be done
        1 => Ok(Some(devices.remove(0))),
        // Otherwise, we've got more than one, so ask the user to make a choice
        _ => {
            // Map the device list to create selection items
            let items: Vec<_> = devices
                .iter()
                .flat_map(
                    |device| -> Result<String, Error> {
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

fn parse_firmware_identity(identity: &String) -> ProbeIdentity
{
    let mut probe = None;
    let mut version = None;

    // BMD product strings are in one ofthe following forms:
    // Recent: Black Magic Probe v2.0.0-rc2
    //       : Black Magic Probe (ST-Link v2) v1.10.0-1273-g2b1ce9aee
    //    Old: Black Magic Probe
    // From this we want to extract two main things: version (if available), and probe variety
    // (probe variety meaning alternative platform kind if not a BMP itself)

    // Let's start out easy - check to see if the string contains an opening paren (alternate platform)
    let opening_paren = identity[BMP_PRODUCT_STRING_LENGTH..].find('(');
    match opening_paren {
        // If there isn't one, we're dealing with nominally a native probe
        None => {
            // Knowing this, let's see if there are enough characters for a version string, and if there are.. extract it
            if identity.len() > BMP_PRODUCT_STRING_LENGTH {
                let version_begin = unsafe { identity.rfind(' ').unwrap_unchecked() };
                version = Some(identity[version_begin + 1..].to_string());
            }
            probe = Some("native".into());
        },
        Some(opening_paren) => {
            let closing_paren = identity[opening_paren..].find(')');
            match closing_paren {
                None => error!("Product description for device is invalid, found opening '(' but no closing ')'"),
                Some(closing_paren) => {
                    // If we did find the closing ')', then see if we've got a version string
                    let version_begin = identity[closing_paren..].find(' ');
                    // If we do, then extract whatever's left of the string as the version number
                    if let Some(version_begin) = version_begin {
                        version = Some(identity[version_begin..].to_string());
                    }
                    // Now we've dealth with the version information, grab everything inside the ()'s as the
                    // product string for this probe (normalised to lower case)
                    probe = Some(identity[opening_paren + 1..closing_paren].to_lowercase());
                }
            }
        },
    };

    ProbeIdentity { probe, version }
}

fn pick_release<'a>(metadata: &'a Metadata, variant: &Probe, firmware_version: &String) ->
    Result<Option<&'a Firmware>, Error>
{
    // Filter out releases that don't support this probe, and filter out the one the probe is currently running
    let releases: Vec<_> = metadata.releases
        .iter()
        .filter(
            |&(version, release)|
                firmware_version != version && release.firmware.contains_key(&variant)
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
    Ok(Some(&metadata.releases[items[selection].as_str()].firmware[&variant]))
}

fn pick_firmware(firmware: &Firmware) -> Result<Option<&FirmwareDownload>, Error>
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
            // Map the variant list to create selection items
            let items: Vec<_> = firmware.variants
                .iter()
                .map(|(_, variant)| variant.friendly_name.as_str())
                .collect();

            // Figure out which one the user wishes to use
            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Which firmware variant would you like to run on your probe?")
                .items(items.as_slice())
                .interact_opt()?;
            // Extract and return that one, if the user didn't cancel selection
            Ok(
                selection
                    .map(|index| items[index])
                    .and_then(
                        |friendly_name| {
                            firmware.variants
                                .iter()
                                .find(|(_, variant)| variant.friendly_name == friendly_name)
                        }
                    )
                    .map(|(_, variant)| variant)
            )
        }
    }
}

impl ProbeIdentity
{
    fn variant(&self) -> Result<Probe, Error>
    {
        match &self.probe {
            Some(product) => {
                let probe = match product.as_str() {
                    "96b carbon" => Probe::_96bCarbon,
                    "blackpill-f401cc" => Probe::BlackpillF401CC,
                    "blackpill-f401ce" => Probe::BlackpillF401CE,
                    "blackpill-f411ce" => Probe::BlackpillF411CE,
                    "ctxlink" => Probe::CtxLink,
                    "f072-if" => Probe::F072,
                    "f3-if" => Probe::F3,
                    "f4discovery" => Probe::F4Discovery,
                    "hydrabus" => Probe::HydraBus,
                    "launchpad icdi" => Probe::LaunchpadICDI,
                    "native" => Probe::Native,
                    "st-link/v2" => Probe::Stlink,
                    "st-link v3" => Probe::Stlinkv3,
                    "swlink" => Probe::Swlink,
                    _ => return Err(ErrorKind::DeviceSeemsInvalid("unknown product string encountered".into()).error()),
                };
                Ok(probe)
            },
            None => Err(ErrorKind::DeviceSeemsInvalid("invalid product string".into()).error()),
        }
    }
}
