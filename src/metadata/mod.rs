pub mod structs;

use std::fs::File;

use log::info;

use crate::error::{Error, ErrorKind};
use crate::metadata::structs::Metadata;

pub fn download_metadata() -> Result<Metadata, Error>
{
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
