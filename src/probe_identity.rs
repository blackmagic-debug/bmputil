use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;
use color_eyre::eyre;
use color_eyre::eyre::eyre;
use dialoguer::Select;
use dialoguer::theme::ColorfulTheme;
use indicatif::ProgressBar;
use log::error;
use crate::firmware_selector::FirmwareMultichoice;
use crate::metadata::structs::{Firmware, FirmwareDownload, Metadata, Probe};

const BMP_PRODUCT_STRING: &str = "Black Magic Probe";
const BMP_PRODUCT_STRING_LENGTH: usize = BMP_PRODUCT_STRING.len();

pub struct ProbeIdentity
{
    probe: Option<String>,
    pub version: Option<String>,
}

impl From<String> for ProbeIdentity {
    fn from(identity: String) -> Self {
        let mut probe = None;
        let mut version = None;

        // BMD product strings are in one of the following forms:
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
                // Knowing this, let's see if there are enough characters for a version string, and if there are, extract it.
                if identity.len() > BMP_PRODUCT_STRING_LENGTH {
                    let version_begin = identity.rfind(' ').expect("There should be enough chars to find the space");
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
                            version = Some(identity[closing_paren + version_begin + 1..].to_string());
                        }
                        // Now we've dealt with the version information, grab everything inside the ()'s as the
                        // product string for this probe (normalised to lower case)
                        probe = Some(identity[BMP_PRODUCT_STRING_LENGTH + opening_paren + 1 ..=closing_paren].to_lowercase());
                    }
                }
            },
        };

        ProbeIdentity { probe, version }
    }
}

impl ProbeIdentity
{
    pub fn variant(&self) -> color_eyre::Result<Probe>
    {
        match &self.probe {
            Some(product) => product.as_str().try_into(),
            None => Err(eyre!("Probe has invalid product string")),
        }
    }

    pub fn version(&self) -> &str {
        // If we don't know what version of firmware is on the probe, presume it's v1.6 for now.
        // We can't actually know which prior version to v1.6 it actually is, but it's very old either way
        match &self.version {
            Some(version) => version,
            None => "v1.6",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_native(){
        let res: ProbeIdentity = "Black Magic Probe v2.0.0-rc2".to_string().into();

        assert_eq!(res.probe, Some("native".into()));
        assert_eq!(res.version, Some("v2.0.0-rc2".into()));
    }

    #[test]
    fn extract_old(){
        let res: ProbeIdentity = "Black Magic Probe".to_string().into();

        assert_eq!(res.probe, Some("native".into()));
        assert_eq!(res.version, None);
    }

    #[test]
    fn extract_st_link(){
        let res: ProbeIdentity = "Black Magic Probe (ST-Link v2) v1.10.0-1273-g2b1ce9aee".to_string().into();

        assert_eq!(res.probe, Some("st-link v2".into()));
        assert_eq!(res.version, Some("v1.10.0-1273-g2b1ce9aee".into()));
    }

    #[test]
    fn extract_without_closing(){
        let res: ProbeIdentity = "Black Magic Probe (ST-Link".to_string().into();

        assert_eq!(res.probe, None);
        assert_eq!(res.version, None);
    }
}
