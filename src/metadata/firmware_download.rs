// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>
// SPDX-FileContributor: Modified by P-Storm <pauldeman@gmail.com>

use std::path::PathBuf;
use color_eyre::eyre::ContextCompat;
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

impl FirmwareDownload {
    pub fn calculate_documentation_url(&self) -> color_eyre::Result<Url>
    {
        let variant_uri = &self.uri;

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


    pub fn calculate_release_uri(&self, release: &str) -> color_eyre::Result<Url>
    {
        // Find where the release tag component is in the path, stripping back to that
        let mut path_segments = self.uri.path_segments().context( "cannot be base")?
            .collect::<Vec<_>>();

        // Find the release segment position
        let release_segment_position = path_segments
            .iter()
            .position(|s| s.ends_with(release))
            .with_context(|| format!("This firmware URL doesn't contain the segment release with value '{}'", release))?;

        let new_segments = path_segments.as_mut_slice()
            .get_mut(..=release_segment_position)
            .context("The segment range should be in path_segment")?;

        let tag_segment_index = release_segment_position.checked_sub(1)
            .with_context(|| format!("Version '{}' segment can't be first segment", release))?;

        //Change the 'download' segment into a 'tag'
        let download_segment = new_segments.get_mut(tag_segment_index).expect("Segment shouldn't be possible to be out of bounds");
        *download_segment = "tag";

        // Only parse the origin
        let mut new_url = Url::parse(&self.uri.origin().ascii_serialization())?;
        {
            let mut path_segments_mut = new_url.path_segments_mut().expect("Cannot be base URL");
            path_segments_mut.clear();
            path_segments_mut.extend(new_segments);
        }

        Ok(new_url)
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

        let res = variant.calculate_release_uri("v1.10.0");

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

        let res = variant.calculate_release_uri("error");

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

        let res = variant.calculate_release_uri("v1.2.3");

        //Can't do Err(err) because of '`'the foreign item type `ErrReport` doesn't implement `PartialEq`'
        assert_eq!(res.unwrap_err().to_string(), "Version 'v1.2.3' segment can't be first segment");
    }

    #[test]
    fn calculate_documentation_url_success()
    {
        let variant = FirmwareDownload{
            friendly_name: "Black Magic Debug for BMP (common targets)".to_string(),
            file_name: PathBuf::from("blackmagic-native-common-v2.0.0-rc1.elf"),
            uri: Url::parse("https://github.com/blackmagic-debug/blackmagic/releases/download/v2.0.0-rc1/blackmagic-native-v2_0_0-rc1.elf").expect("Setup url shouldn't fail"),
        };

        let res = variant.calculate_documentation_url();

        //Can't do Ok(Url) because of '`'the foreign item type `ErrReport` doesn't implement `PartialEq`'
        assert_eq!(res.unwrap(), Url::parse("https://github.com/blackmagic-debug/blackmagic/releases/download/v2.0.0-rc1/blackmagic-native-v2_0_0-rc1.md").unwrap());
    }
}
