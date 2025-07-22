// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>
// SPDX-FileContributor: Modified by P-Storm <pauldeman@gmail.com>

use std::ffi::OsStr;
use std::path::PathBuf;

use color_eyre::Result;
use color_eyre::eyre::eyre;
use serde::Deserialize;
use url::Url;

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct FirmwareDownload
{
	#[serde(rename = "friendlyName")]
	pub friendly_name: String,
	#[serde(rename = "fileName")]
	pub file_name: PathBuf,
	pub uri: Url,
}

impl FirmwareDownload
{
	pub fn build_documentation_url(&self) -> Result<Url>
	{
		// Convert the path compoment of the download URI to a Path
		let mut docs_path = PathBuf::from(&self.uri.path());
		let checked_extension = OsStr::new("elf");
		if docs_path.extension() == Some(checked_extension) {
			docs_path.set_extension("md");
		} else {
			return Err(eyre!(
				"Path extension is not of '{:?}', got '{:?}'",
				checked_extension,
				docs_path.extension()
			));
		}

		// Copy only the origin
		let mut docs_uri = self.uri.clone();
		docs_uri.set_path(docs_path.to_str().expect("Can't set a path from a doc path"));
		docs_uri.set_fragment(None);
		docs_uri.set_query(None);

		Ok(docs_uri)
	}

	pub fn build_release_uri(&self, release: &str) -> Result<Url>
	{
		// Expected uri input: https://github.com/blackmagic-debug/blackmagic/releases/download/<release>/blackmagic-native-v2_0_0-rc1.elf
		let release_position_option = self
			.uri
			.path_segments()
			.expect("URI shape incorrect, must be a web address")
			.position(|r| r == release);

		// The download position should be before the release position
		let download_position = match release_position_option {
			Some(position) => position
				.checked_sub(1)
				.ok_or_else(|| eyre!("The release segment '{}' can't be the first one", release))?,
			None => return Err(eyre!("The provided uri doesn't contain the release segment '{}'", release)),
		};

		// Take the segments up to the /download/ position
		// From: /blackmagic-debug/blackmagic/releases/download/<release>/blackmagic-native-v2_0_0-rc1.elf
		// To  : /blackmagic-debug/blackmagic/releases/tag/<release>
		let release_path: PathBuf = self.uri.path_segments()
			.expect("URI shape incorrect, must be a web address")
			.take(download_position)
			// Now add on /tag/ and the release number
			.chain(["tag", release])
			.collect();

		let new_path = release_path.to_str().expect("cannot be base");

		let mut new_url = self.uri.clone();
		new_url.set_path(new_path);
		new_url.set_fragment(None);
		new_url.set_query(None);

		Ok(new_url)
	}
}

#[cfg(test)]
mod tests
{
	use super::*;

	#[test]
	fn calculate_release_uri_success()
	{
		let variant = FirmwareDownload{
            friendly_name: "Black Magic Debug for BMP (full)".into(),
            file_name: PathBuf::from("blackmagic-native-full-v1.10.0.elf"),
            uri: Url::parse("https://github.com/blackmagic-debug/blackmagic/releases/download/v1.10.0/blackmagic-native-v1_10_0.elf").expect("Setup url shouldn't fail"),
        };

		let res = variant.build_release_uri("v1.10.0");

		// Can't do Ok(Url) because of '`'the foreign item type `ErrReport` doesn't implement `PartialEq`'
		match res {
			Ok(url) => assert_eq!(
				url,
				Url::parse("https://github.com/blackmagic-debug/blackmagic/releases/tag/v1.10.0").unwrap()
			),
			Err(_) => assert!(false, "Shouldn't return an error"),
		}
	}

	#[test]
	fn calculate_release_uri_error()
	{
		let variant = FirmwareDownload {
			friendly_name: "Black Magic Debug for BMP (full)".into(),
			file_name: PathBuf::from("blackmagic-native-full-v1.10.0.elf"),
			uri: Url::parse(
				"https://github.com/blackmagic-debug/blackmagic/releases/v1.10.0/blackmagic-native-v1_10_0.elf",
			)
			.expect("Setup url shouldn't fail"),
		};

		let res = variant.build_release_uri("error");

		// Can't do Err(err) because of '`'the foreign item type `ErrReport` doesn't implement `PartialEq`'
		match res {
			Ok(_) => assert!(false, "Result should fail"),
			Err(str) => assert_eq!(str.to_string(), "The provided uri doesn't contain the release segment 'error'"),
		}
	}

	#[test]
	fn calculate_release_uri_release_first_segment_error()
	{
		let variant = FirmwareDownload {
			friendly_name: "Black Magic Debug for BMP (full)".into(),
			file_name: PathBuf::from("blackmagic-native-full-v1.10.0.elf"),
			uri: Url::parse("https://github.com/v1.2.3").expect("Setup url shouldn't fail"),
		};

		let res = variant.build_release_uri("v1.2.3");

		// Can't do Err(err) because of '`'the foreign item type `ErrReport` doesn't implement `PartialEq`'
		match res {
			Ok(_) => assert!(false, "Result should fail"),
			Err(str) => assert_eq!(str.to_string(), "The release segment 'v1.2.3' can't be the first one"),
		}
	}

	#[test]
	fn calculate_documentation_url_success()
	{
		let variant = FirmwareDownload{
            friendly_name: "Black Magic Debug for BMP (common targets)".into(),
            file_name: PathBuf::from("blackmagic-native-common-v2.0.0-rc1.elf"),
            uri: Url::parse("https://github.com/blackmagic-debug/blackmagic/releases/download/v2.0.0-rc1/blackmagic-native-v2_0_0-rc1.elf").expect("Setup url shouldn't fail"),
        };

		let res = variant.build_documentation_url();

		// Can't do Ok(Url) because of '`'the foreign item type `ErrReport` doesn't implement `PartialEq`'
		match res {
			Ok(url) => assert_eq!(url, Url::parse("https://github.com/blackmagic-debug/blackmagic/releases/download/v2.0.0-rc1/blackmagic-native-v2_0_0-rc1.md").unwrap()),
			Err(_) => assert!(false, "Shouldn't return an error"),
		}
	}
}
