pub mod structs;

use std::fs::File;
use std::io;
use std::path::Path;

use log::info;
use sha2::digest::DynDigest;
use sha2::{Digest, Sha256};

use crate::error::{Error, ErrorKind};
use crate::metadata::structs::Metadata;

pub fn download_metadata(cache: &Path) -> Result<Metadata, Error>
{
	let metadata_file_name = cache.join("metadata.json");
	let etag = compute_etag(metadata_file_name.as_path())?;

	let file = File::open("metadata.json")?;
	// Try to deserialise some datadata
	let metadata: Metadata = serde_json::from_reader(file)?;

	// Having done so, and assuming that didn't blow up, validate the metadata is of a suitable
	// version number, and if it is, dispatch to the appropriate handler for the metadata
	match metadata.version {
		1 => handle_v1_metadata(metadata),
		_ => Err(Error::new(ErrorKind::ReleaseMetadataInvalid, None)),
	}
}

// Handle validation of v1 metadata, prior to letting it return from the download function
fn handle_v1_metadata(metadata: Metadata) -> Result<Metadata, Error>
{
	info!("Validating v1 metadata with {} releases present", metadata.releases.len());
	// Run through the releases in this metadata index
	for (_, release) in &metadata.releases {
		// If they say they include BMDA but they don't, error
		if release.includes_bmda && release.bmda.is_none() {
			return Err(Error::new(ErrorKind::ReleaseMetadataInvalid, None));
		}
		// If they say they do not include BMDA but they do, error
		if !release.includes_bmda && release.bmda.is_some() {
			return Err(Error::new(ErrorKind::ReleaseMetadataInvalid, None));
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

fn compute_etag(metadata_file_name: &Path) -> Result<String, Error>
{
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
	Ok(result)
}
