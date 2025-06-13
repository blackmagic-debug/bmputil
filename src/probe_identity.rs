use std::fmt::{Display, Formatter};
use color_eyre::eyre::{eyre, Result};
use color_eyre::Report;

use crate::metadata::structs::Probe;

const BMP_PRODUCT_STRING: &str = "Black Magic Probe";
const BMP_PRODUCT_STRING_LENGTH: usize = BMP_PRODUCT_STRING.len();
const BMP_NATIVE: &str = "native";

#[derive(Debug, PartialEq, Eq)]
pub enum Version
{
    Unknown,
    Known(String),
}

#[derive(Debug, PartialEq, Eq)]
pub struct ProbeIdentity
{
    probe: Probe,
    pub version: Version,
}

enum ParseNameError
{
    OpeningParenthesisAfterClosingParenthesis,
    FoundNotMatchedParenthesis
}

impl Display for ParseNameError
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result
    {
        match self {
            ParseNameError::OpeningParenthesisAfterClosingParenthesis => write!(f, "A '(' parenthesis is found after a ')'."),
            ParseNameError::FoundNotMatchedParenthesis => write!(f, "Not a matching pair of parenthesis found."),
        }
    }
}

#[derive(Debug)]
enum ParseVersionError
{
    FormattingPatternError,
    EmptyOrWhitespaceVersion,
    NotStartingWithV(String),
}

impl Display for ParseVersionError
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result
    {
        match self {
            ParseVersionError::FormattingPatternError => write!(f, "The version failed to match the pattern of '{} (<version number>)'.", BMP_PRODUCT_STRING),
            ParseVersionError::EmptyOrWhitespaceVersion => write!(f, "The extracted version is empty or whitespace"),
            ParseVersionError::NotStartingWithV(version_string) => write!(f, "Version doesn't start with v, got '{}'", version_string),
        }
    }
}

fn parse_name_from_identity_string(input: &str) -> Result<String, ParseNameError>
{
    let opening_paren = input.find('(');
    let closing_paren = input.find(')');

    match (opening_paren, closing_paren) {
        (None, None) => Ok(BMP_NATIVE.into()),
        (Some(opening_paren), Some(closing_paren)) => {
            if opening_paren > closing_paren {
                Err(ParseNameError::OpeningParenthesisAfterClosingParenthesis)
            } else {
               Ok(input[opening_paren+1..closing_paren].to_string())
            }
        }
        (Some(_), None) => Err(ParseNameError::FoundNotMatchedParenthesis),
        (None, Some(_)) => Err(ParseNameError::FoundNotMatchedParenthesis),
    }
}

fn parse_version_from_identity_string(input: &str) -> Result<Version, ParseVersionError>
{
    let start_index = input.rfind(' ').ok_or_else(|| ParseVersionError::FormattingPatternError)?;

    let version = input[start_index + 1..].to_string();

    if !version.starts_with('v') {
        return Err(ParseVersionError::NotStartingWithV(version));
    }
    
    if version.trim().is_empty() {
        return Err(ParseVersionError::EmptyOrWhitespaceVersion);
    }

    Ok(Version::Known(version))
}

impl TryFrom<String> for ProbeIdentity
{
    type Error = Report;

    // BMD product strings are in one of the following forms:
    // Recent: Black Magic Probe v2.0.0-rc2
    //       : Black Magic Probe (ST-Link v2) v1.10.0-1273-g2b1ce9aee
    //       : Black Magic Probe v2.0.0-rc2-65-g221c3031f
    //    Old: Black Magic Probe

    fn try_from(value: String) -> Result<Self>
    {
        let identity = value;

        // Every identity should start with 'Black Magic Probe'
        if !identity.starts_with(BMP_PRODUCT_STRING) {
            return Err(eyre!("Product string doesn't start with '{}'", BMP_PRODUCT_STRING));
        }

        // If it is exactly 'Black Magic Probe', then it is an old identity, with an unknown version.
        if identity == BMP_PRODUCT_STRING {
            return Ok(ProbeIdentity {
                probe: Probe::Native,
                version: Version::Unknown,
            })
        }

        //Removes the first length from the identity, because we know it starts with the 'Black Magic Probe'
        let parse_slice = &identity[BMP_PRODUCT_STRING_LENGTH..];
        let probe_result = parse_name_from_identity_string(parse_slice);
        let probe_string = probe_result.or_else(|error| Err(eyre!("Error while parsing probe string: {}", error)))?;
        let probe = probe_string.to_lowercase().try_into()?;

        let version_result = parse_version_from_identity_string(parse_slice);
        let version = version_result.or_else(|error| Err(eyre!("Error while parsing version string: {}", error)))?;
        Ok(ProbeIdentity {
            probe,
            version
        })
    }
}

impl ProbeIdentity
{
    pub fn variant(&self) -> Probe
    {
        self.probe
    }
}

impl TryFrom<String> for Probe
{
    type Error = Report;

    fn try_from(value: String) -> Result<Self>
    {
        match value.as_str() {
            "96b carbon" => Ok(Probe::_96bCarbon),
            "blackpill-f401cc" => Ok(Probe::BlackpillF401CC),
            "blackpill-f401ce" => Ok(Probe::BlackpillF401CE),
            "blackpill-f411ce" => Ok(Probe::BlackpillF411CE),
            "ctxlink" => Ok(Probe::CtxLink),
            "f072-if" => Ok(Probe::F072),
            "f3-if" => Ok(Probe::F3),
            "f4discovery" => Ok(Probe::F4Discovery),
            "hydrabus" => Ok(Probe::HydraBus),
            "launchpad icdi" => Ok(Probe::LaunchpadICDI),
            BMP_NATIVE => Ok(Probe::Native),
            "st-link/v2" => Ok(Probe::Stlink),
            "st-link v3" => Ok(Probe::Stlinkv3),
            "swlink" => Ok(Probe::Swlink),
            _ => Err(eyre!("Probe with unknown product string encountered")),
        }
    }
}
