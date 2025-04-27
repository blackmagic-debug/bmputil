// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use color_eyre::eyre::{eyre, Result};
use dialoguer::theme::ColorfulTheme;
use dialoguer::Select;

use crate::metadata::structs::FirmwareDownload;

pub struct FirmwareMultichoice<'a>
{
    state: State,
    release: &'a str,
    variants: Vec<&'a FirmwareDownload>,
    friendly_names: Vec<&'a str>,
}

#[derive(Default)]
enum State
{
    #[default]
    PickFirmware,
    PickAction(usize),
    ShowDocs(usize, usize),
    FlashFirmware(usize),
    Cancel,
}

impl<'a> FirmwareMultichoice<'a>
{
    pub fn new(release: &'a str, variants: &'a BTreeMap<String, FirmwareDownload>) -> Self
    {
        // Map the variant list to create selection items
        let friendly_names: Vec<_> = variants
            .iter()
            .map(|(_, variant)| variant.friendly_name.as_str())
            .collect();

        // Construct the new multi-choice object that will start in the default firmware selection state
        Self {
            state: State::default(),
            release,
            variants: variants.values().collect(),
            friendly_names
        }
    }

    /// Returns true if the FSM is finished and there are no further state transitions to go
    pub fn complete(&self) -> bool
    {
        match self.state {
            State::FlashFirmware(_) | State::Cancel => true,
            _ => false,
        }
    }

    /// Step the FSM and perform the actions associated with that step
    pub fn step(&mut self) -> Result<()>
    {
        self.state = match self.state {
            State::PickFirmware => self.firmware_selection()?,
            State::PickAction(index) => self.action_selection(index)?,
            State::ShowDocs(name_index, variant_index) =>
                self.show_documentation(name_index, variant_index)?,
            // FlashFirmware and Cancel are both terminal actions whereby the FSM is done,
            // so maintain homeostatis for them here.
            State::FlashFirmware(index) => State::FlashFirmware(index),
            State::Cancel => State::Cancel,
        };

        Ok(())
    }

    /// Convert the FSM state into a firmware download selection
    pub fn selection(&self) -> Option<&'a FirmwareDownload>
    {
        match self.state {
            State::FlashFirmware(index) => Some(self.variants[index]),
            _ => None,
        }
    }

    fn firmware_selection(&self) -> Result<State>
    {
        // Figure out which one the user wishes to use
        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Which firmware variant would you like to run on your probe?")
            .items(self.friendly_names.as_slice())
            .interact_opt()?;
        // Encode the result into a new FSM state
        Ok(match selection {
            Some(index) => State::PickAction(index),
            None => State::Cancel,
        })
    }

    fn action_selection(&self, name_index: usize) -> Result<State>
    {
        // Convert from a friendly name index into the matching variant download
        let friendly_name = self.friendly_names[name_index];
        let (variant_index, _) = self.variants
            .iter()
            .enumerate()
            .find(|(_, variant)| variant.friendly_name == friendly_name)
            .unwrap(); // Can't fail anyway..

        // Ask the user what they wish to do
        let items = ["Flash to probe", "Show documentation", "Choose a different variant"];
        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("What action would you like to take with this firmware?")
            .items(&items)
            .interact_opt()?;

        Ok(match selection {
            Some(item) => match item {
                0 => State::FlashFirmware(variant_index),
                1 => State::ShowDocs(name_index, variant_index),
                2 => State::PickFirmware,
                _ => Err(eyre!("Impossible selection for action"))?
            },
            None => State::Cancel,
        })
    }

    fn show_documentation(&self, name_index: usize, variant_index: usize) -> Result<State>
    {
        // Extract which firmware download we're to work with
        let variant = self.variants[variant_index];
        // Convert the path compoment of the download URI to a Path
        let mut docs_path = PathBuf::from(variant.uri.path());
        // Replace the file extension from ".elf" to ".md"
        docs_path.set_extension("md");
        // Convert back into a URI
        let mut docs_uri = variant.uri.clone();
        docs_uri.set_path(
            docs_path.to_str()
                .expect("Something went terribly wrong building the documentation URI")
        );

        // Now try and download this documentation file
        let client = reqwest::blocking::Client::new();
        let response = client.get(docs_uri)
            // Use a 2 second timeout so we don't get stuck forever if the user is
            // having connectivity problems - better to die early and have them retry
            .timeout(Duration::from_secs(2))
            .send()?;

        Ok(State::PickAction(name_index))
    }
}
