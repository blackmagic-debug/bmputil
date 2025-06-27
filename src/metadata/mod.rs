// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Writen by Rachel Mant <git@dragonmux.network>

pub mod firmware_download;
pub mod structs;

use std::fs::{File, create_dir_all};
use std::io;
use std::path::Path;
use std::time::Duration;

use color_eyre::eyre::Result;
use indicatif::ProgressBar;
use log::info;
use reqwest::StatusCode;
use sha2::digest::DynDigest;
use sha2::{Digest, Sha256};

use crate::error::ErrorKind;
use crate::metadata::structs::Metadata;

pub fn download_metadata(cache: &Path) -> Result<Metadata>
{
	// Compute the name of the metadata file in the cache and discover its ETag
	let metadata_file_name = cache.join("metadata.json");
	let etag = compute_etag(metadata_file_name.as_path())?;

	// Set up a progress ticker so the user knows something is happening
	let progress = ProgressBar::new_spinner().with_message("Updating release metadata cache");
	// Tick the spinner once every 100ms so we get a smooth showing of progress
	progress.enable_steady_tick(Duration::from_millis(100));

	// Put together a request to summon to get any updates needed to the metadata
	let client = reqwest::blocking::Client::new();
	let mut request = client.get("https://summon.black-magic.org/metadata.json")
	// Use a 2 second timeout so we don't get stuck forever if the user is
	// having connectivity problems - better to die early and have them retry
		.timeout(Duration::from_secs(2));
	if let Some(etag) = etag {
		request = request.header("If-None-Match", etag);
	}
	let mut response = request.send()?;
	// See if the response was good - if it was, put the result into the cache
	if response.status() == StatusCode::OK {
		create_dir_all(cache)?;
		let mut metadata_file = File::create(metadata_file_name.as_path())?;
		response.copy_to(&mut metadata_file)?;
	// If the response was anything other than 200 or 304
	} else if response.status() != StatusCode::NOT_MODIFIED {
		progress.finish();
		return Err(ErrorKind::ReleaseMetadataInvalid.error().into());
	}
	// Finish the progress spinner so the user sees the download finished
	progress.finish();

	// Now try to open the file and deserialise some metadata from it
	let file = File::open(metadata_file_name)?;
	let metadata: Metadata = serde_json::from_reader(file)?;

	// Having done so, and assuming that didn't blow up, validate the metadata is of a suitable
	// version number, and if it is, dispatch to the appropriate handler for the metadata
	match metadata.version {
		1 => handle_v1_metadata(metadata),
		_ => Err(ErrorKind::ReleaseMetadataInvalid.error().into()),
	}
}

// Handle validation of v1 metadata, prior to letting it return from the download function
fn handle_v1_metadata(metadata: Metadata) -> Result<Metadata>
{
	info!("Validating v1 metadata with {} releases present", metadata.releases.len());
	// Run through the releases in this metadata index
	for release in metadata.releases.values() {
		// If they say they include BMDA but they don't, error
		if release.includes_bmda && release.bmda.is_none() {
			return Err(ErrorKind::ReleaseMetadataInvalid.error().into());
		}
		// If they say they do not include BMDA but they do, error
		if !release.includes_bmda && release.bmda.is_some() {
			return Err(ErrorKind::ReleaseMetadataInvalid.error().into());
		}
	}
	Ok(metadata)
}

fn hex_digit(value: u8) -> char
{
	// Copy the digit to work on
	let mut digit = value;
	// If this digit (which must be a nibble between 0 and 15!) is more than 9, set up
	// to convert it to a lower case letter (a-f)
	if value > 9 {
		// 'a' - '0' - 10
		digit += 0x61 - 0x30 - 10;
	}
	// Now the digit is set up to either convert to a number of a letter, add '0' to do that
	digit += 0x30;
	char::from(digit)
}

fn compute_etag(metadata_file_name: &Path) -> Result<Option<String>>
{
	// Check if the metadata file is a thing to start with
	if !metadata_file_name.exists() {
		// If it does not exist, then there's no ETag - we need to download this fresh.
		return Ok(None);
	}
	// Open the file to hash, and make a SHA256 hashing instance for it
	let mut file = File::open(metadata_file_name)?;
	let mut hasher = Sha256::default();
	// Grab how many bytes it takes to represent these hashesh
	let hash_length = hasher.output_size();
	// Put the file contents through the hashing algorithm, and extract the resulting hash
	io::copy(&mut file, &mut hasher)?;
	let hash = hasher.finalize();
	// Make a new String for our result to go into as a series of hex digits (so 2x the hash length)
	// But don't forget to also inclue the space for the double quotes ETags required
	let mut result = String::with_capacity((hash_length * 2) + 2);
	result.push('"');
	// Grab the hash bytes one by one
	for byte in hash {
		// Convert the byte into its hex digits
		result.push(hex_digit(byte >> 4));
		result.push(hex_digit(byte & 0x0f));
	}
	result.push('"');
	Ok(Some(result))
}
