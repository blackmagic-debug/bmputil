// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use clap::ArgMatches;
use dialoguer::Select;

use crate::bmp::BmpDevice;
use crate::bmp::BmpMatcher;
use crate::error::Error;

pub fn switch_firmware(matches: &ArgMatches) -> Result<(), Error>
{
    // Start by figuring out which probe to use for the operation
    let probe = select_probe(matches)?;
    println!("Probe {} selected for firmware update", probe);

    Ok(())
}

fn select_probe(matches: &ArgMatches) -> Result<BmpDevice, Error>
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
        1 => Ok(devices.remove(0)),
        // Otherwise, we've got more than one, so ask the user to make a choice
        _ => {
            // Figure out which one the user wishes to use
            let selection = Select::new()
                .with_prompt("Which probe would you like to change the firmware on?")
                .items(devices.as_slice())
                .interact()?;
            // Extract and return that one
            Ok(devices.remove(selection))
        },
    }
}
