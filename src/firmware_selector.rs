// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>
// SPDX-FileContributor: Modified by P-Storm <pauldeman@gmail.com>

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use color_eyre::eyre::{Result, eyre, ContextCompat};
use dialoguer::Select;
use dialoguer::theme::ColorfulTheme;
use reqwest::StatusCode;
use url::Url;

use crate::docs_viewer::Viewer;
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
			friendly_names,
		}
	}

	/// Returns true if the FSM is finished and there are no further state transitions to go
	pub fn complete(&self) -> bool
	{
		matches!(self.state, State::FlashFirmware(_) | State::Cancel)
	}

	/// Step the FSM and perform the actions associated with that step
	pub fn step(&mut self) -> Result<()>
	{
		self.state = match self.state {
			State::PickFirmware => self.firmware_selection()?,
			State::PickAction(index) => self.action_selection(index)?,
			State::ShowDocs(name_index, variant_index) => self.show_documentation(name_index, variant_index)?,
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
		let (variant_index, _) = self
			.variants
			.iter()
			.enumerate()
			.find(|(_, variant)| variant.friendly_name == friendly_name)
			.expect("The friendly_name should always be found");

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
				_ => Err(eyre!("Impossible selection for action"))?,
			},
			None => State::Cancel,
		})
	}

	fn calculate_release_uri(&self, variant: &FirmwareDownload) -> Result<Url>
	{
		// Find where the release tag component is in the path, stripping back to that
		let mut path_segments = variant.uri.path_segments().context( "cannot be base")?
			.collect::<Vec<_>>();

		// Find the release segment position
		let release_segment_position = path_segments
			.iter()
			.position(|s| s.ends_with(self.release))
			.with_context(|| format!("This firmware URL doesn't contain the segment release with value '{}'", self.release))?;
		
		let new_segments = path_segments.as_mut_slice()
			.get_mut(..=release_segment_position)
			.context("The segment range should be in path_segment")?;

		let tag_segment_index = release_segment_position.checked_sub(1)
			.with_context(|| format!("Version '{}' segment can't be first segment", self.release))?;
		
		//Change the 'download' segment into a 'tag'
		let download_segment = new_segments.get_mut(tag_segment_index).expect("Segment shouldn't be possible to be out of bounds");
		*download_segment = "tag";
		
		// Only parse the origin
		let mut new_url = Url::parse(&variant.uri.origin().ascii_serialization())?;
		{
			let mut path_segments_mut = new_url.path_segments_mut().expect("Cannot be base URL");
			path_segments_mut.clear(); 
			path_segments_mut.extend(new_segments);
		}

		Ok(new_url)
	}

	fn calculate_documentation_url(&self, variant_uri: &Url) -> Result<Url>
	{
		// Convert the path compoment of the download URI to a Path
		let mut docs_path = PathBuf::from(variant_uri.path());
		// Replace the file extension from ".elf" to ".md"
		docs_path.set_extension("md");
		// Copy only the origin
		let mut docs_uri = Url::parse(&variant_uri.origin().ascii_serialization())?;
		docs_uri.set_path(
			docs_path
				.to_str()
				.expect("Can't set a path from a doc path")
		);

		Ok(docs_uri)
	}

	fn show_documentation(&self, name_index: usize, variant_index: usize) -> Result<State>
	{
		// Extract which firmware download we're to work with
		let variant = self.variants[variant_index];

		// Convert back into a URI
		let docs_uri = self.calculate_documentation_url(&variant.uri)?;

		// Now try and download this documentation file
		let client = reqwest::blocking::Client::new();
		let response = client.get(docs_uri)
            // Use a 2 second timeout so we don't get stuck forever if the user is
            // having connectivity problems - better to die early and have them retry
            .timeout(Duration::from_secs(2))
            .send()?;

		match response.status() {
			// XXX: Need to compute the release URI from the download URI and release name string
			StatusCode::NOT_FOUND => println!(
				"No documentation found, please go to {} to find out more",
				self.calculate_release_uri(variant)?
			),
			StatusCode::OK => Viewer::display(&variant.friendly_name, &response.text()?)?,
			status => Err(eyre!(
				"Something went terribly wrong while grabbing the documentation to display: {}",
				status
			))?,
		};

		Ok(State::PickAction(name_index))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn calculate_release_uri_success()
	{
		let variant = FirmwareDownload{
			friendly_name: "Black Magic Debug for BMP (full)".to_string(),
			file_name: PathBuf::from("blackmagic-native-full-v1.10.0.elf"),
			uri: Url::parse("https://github.com/blackmagic-debug/blackmagic/releases/download/v1.10.0/blackmagic-native-v1_10_0.elf").expect("Setup url shouldn't fail"),
		};

		let map = &BTreeMap::from([
			("full".to_string(), variant.clone())
		]);

		let multiple_choice = FirmwareMultichoice::new("v1.10.0", map);

		let res = multiple_choice.calculate_release_uri(&variant);

		//Can't do Ok(Url) because of '`'the foreign item type `ErrReport` doesn't implement `PartialEq`'
		assert_eq!(res.unwrap(), Url::parse("https://github.com/blackmagic-debug/blackmagic/releases/tag/v1.10.0").unwrap());
	}

	#[test]
	fn calculate_release_uri_error()
	{
		let variant = FirmwareDownload{
			friendly_name: "Black Magic Debug for BMP (full)".to_string(),
			file_name: PathBuf::from("blackmagic-native-full-v1.10.0.elf"),
			uri: Url::parse("https://github.com/blackmagic-debug/blackmagic/releases/download/v1.10.0/blackmagic-native-v1_10_0.elf").expect("Setup url shouldn't fail"),
		};

		let map = &BTreeMap::from([
			("full".to_string(), variant.clone())
		]);

		let multiple_choice = FirmwareMultichoice::new("error", map);

		let res = multiple_choice.calculate_release_uri(&variant);

		//Can't do Err(err) because of '`'the foreign item type `ErrReport` doesn't implement `PartialEq`'
		assert_eq!(res.unwrap_err().to_string(), "This firmware URL doesn't contain the segment release with value 'error'");
	}

	#[test]
	fn calculate_release_uri_release_first_segment_error()
	{
		let variant = FirmwareDownload{
			friendly_name: "Black Magic Debug for BMP (full)".to_string(),
			file_name: PathBuf::from("blackmagic-native-full-v1.10.0.elf"),
			uri: Url::parse("https://github.com/v1.2.3").expect("Setup url shouldn't fail"),
		};

		let map = &BTreeMap::from([
			("full".to_string(), variant.clone())
		]);

		let multiple_choice = FirmwareMultichoice::new("v1.2.3", map);

		let res = multiple_choice.calculate_release_uri(&variant);

		//Can't do Err(err) because of '`'the foreign item type `ErrReport` doesn't implement `PartialEq`'
		assert_eq!(res.unwrap_err().to_string(), "Version 'v1.2.3' segment can't be first segment");
	}

	#[test]
	fn calculate_documentation_url_success(){
		let variant = FirmwareDownload{
			friendly_name: "Black Magic Debug for BMP (common targets)".to_string(),
			file_name: PathBuf::from("blackmagic-native-common-v2.0.0-rc1.elf"),
			uri: Url::parse("https://github.com/blackmagic-debug/blackmagic/releases/download/v2.0.0-rc1/blackmagic-native-v2_0_0-rc1.elf").expect("Setup url shouldn't fail"),
		};

		let map = &BTreeMap::default();

		let multiple_choice = FirmwareMultichoice::new("native", map);
		let res = multiple_choice.calculate_documentation_url(&variant.uri);

		//Can't do Ok(Url) because of '`'the foreign item type `ErrReport` doesn't implement `PartialEq`'
		assert_eq!(res.unwrap(), Url::parse("https://github.com/blackmagic-debug/blackmagic/releases/download/v2.0.0-rc1/blackmagic-native-v2_0_0-rc1.md").unwrap());
	}
}