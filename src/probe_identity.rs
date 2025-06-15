// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>
// SPDX-FileContributor: Modified by P-Storm <pauldeman@gmail.com>

use std::cmp::Ordering;

use std::fmt::{Display, Formatter};
use color_eyre::eyre::{eyre, Report, Result};

use crate::metadata::structs::Probe;

const BMP_PRODUCT_STRING: &str = "Black Magic Probe";
const BMP_PRODUCT_STRING_LENGTH: usize = BMP_PRODUCT_STRING.len();
const BMP_NATIVE: &str = "native";

#[derive(PartialEq, Eq)]
pub struct ProbeIdentity
{
    probe: Probe,
    pub version: VersionNumber,
}

enum ParseNameError
{
    OpeningParenthesisAfterClosingParenthesis,
    FoundNotMatchedParenthesis
}

#[derive(Debug)]
enum ParseVersionError
{
    FormattingPatternError,
    EmptyOrWhitespaceVersion,
}

#[derive(Debug, PartialEq, Eq, Ord)]
pub enum VersionNumber
{
    Unknown,
    Invalid,
    GitHash(String),
    FullVersion(VersionParts),
}

#[derive(Debug, PartialEq, Eq, Ord)]
pub struct VersionParts
{
    major: usize,
    minor: usize,
    revision: usize,
    kind: VersionKind,
    dirty: bool,
}

#[derive(Debug, PartialEq, Eq, Ord)]
pub enum VersionKind
{
    Release,
    ReleaseCandidate(usize),
    Development(GitVersion),
}

#[derive(Debug, PartialEq, Eq, Ord)]
pub struct GitVersion
{
    release_candidate: Option<usize>,
    commits: usize,
    hash: String,
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

impl Display for ParseVersionError
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result
    {
        match self {
            ParseVersionError::FormattingPatternError => write!(f, "The version failed to match the pattern of '{} (<version number>)'.", BMP_PRODUCT_STRING),
            ParseVersionError::EmptyOrWhitespaceVersion => write!(f, "The extracted version is empty or whitespace"),
        }
    }
}

fn parse_name_from_identity_string(input: &str) -> Result<&str, ParseNameError>
{
    let opening_paren = input.find('(');
    let closing_paren = input.find(')');

    match (opening_paren, closing_paren) {
        (None, None) => Ok(BMP_NATIVE.into()),
        (Some(opening_paren), Some(closing_paren)) => {
            if opening_paren > closing_paren {
                Err(ParseNameError::OpeningParenthesisAfterClosingParenthesis)
            } else {
                Ok(&input[opening_paren+1..closing_paren])
            }
        }
        (Some(_), None) => Err(ParseNameError::FoundNotMatchedParenthesis),
        (None, Some(_)) => Err(ParseNameError::FoundNotMatchedParenthesis),
    }
}

fn parse_version_from_identity_string(input: &str) -> Result<&str, ParseVersionError>
{
    let start_index = input.rfind(' ').ok_or_else(|| ParseVersionError::FormattingPatternError)?;
    let version = &input[start_index + 1..];

    if version.trim().is_empty() {
        return Err(ParseVersionError::EmptyOrWhitespaceVersion);
    }

    Ok(version)
}

impl TryFrom<&str> for ProbeIdentity
{
    type Error = Report;

    fn try_from(identity: &str) -> Result<Self>
    {
        // BMD product strings are in one of the following forms:
        // Recent: Black Magic Probe v2.0.0-rc2
        //       : Black Magic Probe (ST-Link/v2) v1.10.0-1273-g2b1ce9aee
        //    Old: Black Magic Probe
        // From this we want to extract two main things: version (if available), and probe variety
        // (probe variety meaning alternative platform kind if not a BMP itself)

        // Every identity should start with 'Black Magic Probe'
        if !identity.starts_with(BMP_PRODUCT_STRING) {
            return Err(eyre!("Product string doesn't start with '{}'", BMP_PRODUCT_STRING));
        }

        // If it is exactly 'Black Magic Probe', then it is an old identity, with an unknown version.
        if identity == BMP_PRODUCT_STRING {
            return Ok(ProbeIdentity {
                probe: Probe::Native,
                version: VersionNumber::Unknown,
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
            version: version.into()
        })
    }
}

impl TryFrom<String> for ProbeIdentity
{
    type Error = Report;

    fn try_from(identity: String) -> Result<Self>
    {
        identity.as_str().try_into()
    }
}

impl Display for ProbeIdentity
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result
    {
        // Probe names always use the product string as a prefix
        write!(f, "{}", BMP_PRODUCT_STRING)?;
        // If it's not a native probe, display the variant name
        if self.probe != Probe::Native {
            write!(f, " ({})", self.probe.to_string())?;
        }
        // Translate the version string as best as possible to a readable form
        match &self.version {
            VersionNumber::Unknown => Ok(()),
            VersionNumber::Invalid => write!(f, " <invalid version>"),
            VersionNumber::GitHash(hash) => write!(f, " {}", hash),
            VersionNumber::FullVersion(version_parts) => write!(f, " {}", version_parts.to_string()),
        }
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

impl From<&str> for VersionNumber
{
    fn from<'a>(value: &str) -> Self
    {
        // Check what the version string starts with - if it starts with a 'g', it's a GitHash, 'v' is a version,
        // anything else is invalid and unknown.
        if value.starts_with("g") {
            VersionNumber::GitHash(value[1..].to_string())
        } else if value.starts_with("v") {
            // Try to convert the version number into parts
            let version_parts = VersionParts::try_from(&value[1..]);
            match version_parts {
                // If that succeeds return a fully versioned object
                Ok(version_parts) => VersionNumber::FullVersion(version_parts),
                // Otherwise it's an invalid version, so chuck back an invalid version object
                Err(_) => VersionNumber::Invalid,
            }
        } else {
            VersionNumber::Invalid
        }
    }
}

impl From<&String> for VersionNumber
{
    fn from(value: &String) -> Self
    {
        value.as_str().into()
    }
}

impl From<String> for VersionNumber
{
    fn from(value: String) -> Self
    {
        value.as_str().into()
    }
}

impl ToString for VersionNumber
{
    fn to_string(&self) -> String
    {
        match self {
            Self::Unknown => "".into(),
            Self::Invalid => "<invalid version>".into(),
            Self::GitHash(git_hash) => git_hash.clone(),
            Self::FullVersion(version_parts) => version_parts.to_string()
        }
    }
}

impl PartialOrd for VersionNumber
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering>
    {
        // There's no ordering invalid version numbers, or Git hash only ones beyond equality
        match self {
            Self::Unknown => None,
            Self::Invalid => None,
            Self::GitHash(lhs) => {
                match other {
                    // If the other number is also a GitHash, check if they're equal.
                    // For everything else, there's no way to compare
                    Self::GitHash(rhs) => {
                        if lhs == rhs {
                            Some(Ordering::Equal)
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            },
            Self::FullVersion(lhs) => {
                // If we're a full version though then if the RHS is also a full version, apply full
                // partial comparison - everything else cannot be ordered however.
                match other {
                    Self::Unknown | Self::Invalid | Self::GitHash(_) => None,
                    Self::FullVersion(rhs) => lhs.partial_cmp(rhs),
                }
            }
        }
    }
}

impl VersionParts
{
    pub fn from_parts(major: usize, minor: usize, revision: usize, kind: VersionKind, dirty: bool) -> Self
    {
        Self { major, minor, revision, kind, dirty }
    }
}

impl TryFrom<&str> for VersionParts
{
    type Error = Report;

    fn try_from(value: &str) -> Result<Self>
    {
        // The caller already chopped the leading `v` off, so..
        // Start by extracting each of the components, one dot at a time.
        // Look for the first '.' and extract the major version number
        let major_end = value.find('.').unwrap_or_else(|| value.len());
        let major = value[..major_end].parse::<usize>()?;

        let mut value = if major_end == value.len() {
            &value[major_end..]
        } else {
            &value[major_end + 1..]
        };

        // Next, find another dot if possible and extract the minor
        let minor_end = value.find('.').unwrap_or_else(|| value.len());
        let minor = value[..minor_end].parse::<usize>()?;

        value = if minor_end == value.len() {
            &value[minor_end..]
        } else {
            &value[minor_end + 1..]
        };

        // And one more time - this time for the revision number, and look for a '-'
        let revision_end = value.find('-').unwrap_or_else(|| value.len());
        let revision = value[..revision_end].parse::<usize>()?;

        value = if revision_end == value.len() {
            &value[revision_end..]
        } else {
            &value[revision_end + 1..]
        };

        // Now look from the end for another '-', this time to see if the dirty marker is set
        let dirty_begin = value
            .rfind('-')
            .map(|value| value + 1)
            .unwrap_or(0);
        let dirty = &value[dirty_begin..] == "dirty";
        // If the marker was present, remove it from the string
        if dirty {
            value = &value[..dirty_begin];
            if value.ends_with('-') {
                value = &value[..value.len() - 1];
            }
        }

        // Depending on how much string is left, we need to do different things here..
        // If there is no string left, the kind is a release and we're done
        let kind = if value.is_empty() {
            VersionKind::Release
        } else {
            // More to come? okay.. let's see if this is a release candidate next then
            let candidate = if value.starts_with("rc") {
                let rc_end = value.find('-').unwrap_or_else(|| value.len());
                let rc_number = value[2..rc_end].parse::<usize>()?;

                value = if rc_end == value.len() {
                    &value[rc_end..]
                } else {
                    &value[rc_end + 1..]
                };

                Some(rc_number)
            } else {
                None
            };
            // If there's anything left, we now have Git version information.
            // First comes the number of commits since the tag this is referenced to was made
            if !value.is_empty() {
                // Find the middle '-' and parse the first part as a number
                let commits_end = value
                    .find('-')
                    .ok_or_else(|| eyre!("Version string has invalid form of Git version tag {:?}", value))?;
                let commits = value[..commits_end].parse::<usize>()?;
                // Now take everything after the '-' as a hash
                let hash = value[commits_end + 1..].to_string();
                VersionKind::Development(GitVersion { commits, hash, release_candidate: candidate })
            } else {
                candidate.map(|rc_number| VersionKind::ReleaseCandidate(rc_number)).unwrap()
            }
        };

        Ok(Self
        {
            major,
            minor,
            revision,
            kind,
            dirty,
        })
    }
}

impl ToString for VersionParts
{
    fn to_string(&self) -> String
    {
        // Build out the base version string
        let mut version = format!("{}.{}.{}", self.major, self.minor, self.revision);
        // Now flatten out the kind value
        version += &self.kind.to_string();
        // And finally, if the version represents a dirty build, add that before we return
        if self.dirty {
            version += "-dirty";
        }
        version
    }
}

impl PartialOrd for VersionParts
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering>
    {
        // First, check to see if the major is larger or smaller than the other
        if self.major < other.major {
            return Some(Ordering::Less);
        } else if self.major > other.major {
            return Some(Ordering::Greater);
        }

        // Next check the minor in the same way
        if self.minor < other.minor {
            return Some(Ordering::Less);
        } else if self.minor > other.minor {
            return Some(Ordering::Greater);
        }

        // Now check the revision number
        if self.revision < other.revision {
            return Some(Ordering::Less);
        } else if self.revision > other.revision {
            return Some(Ordering::Greater);
        }

        // If we got here, the major, minor, and revision numbers all match.. so,
        // we can properly check the ordering on the kind as we're comparing all the same base numbers
        if self.kind < other.kind {
            return Some(Ordering::Less)
        } else if self.kind > other.kind {
            return Some(Ordering::Greater)
        }

        // If the version given is `-dirty`, but other is not, we are a higher version number
        // (and likewise the other way around - other is the higher then). If they're equal,
        // then the version numbers are equivilent.
        if self.dirty && !other.dirty {
            Some(Ordering::Greater)
        } else if !self.dirty && other.dirty {
            Some(Ordering::Less)
        } else {
            Some(Ordering::Equal)
        }
    }
}

impl ToString for VersionKind
{
    fn to_string(&self) -> String
    {
        match self {
            Self::Release => "".into(),
            Self::ReleaseCandidate(rc_number) => format!("-rc{}", rc_number),
            Self::Development(git_version) => git_version.to_string(),
        }
    }
}

impl PartialOrd for VersionKind
{
    /// NB: These orderings are only true IFF we are comparing the same base versions in VersionParts.
    /// a release candidate comes before a release, but development builds come after that release(candidate).
    fn partial_cmp(&self, other: &Self) -> Option<Ordering>
    {
        match self {
            Self::Release => {
                // A release comes after a release candidate but before its development builds
                match other {
                    Self::Release => Some(Ordering::Equal),
                    Self::ReleaseCandidate(_) => Some(Ordering::Greater),
                    Self::Development(_) => Some(Ordering::Less),
                }
            },
            Self::ReleaseCandidate(lhs) => {
                // A release candidate comes before a release and its development builds, but candidates
                // a strongly ordered relative to each other for a given release
                match other {
                    Self::Release => Some(Ordering::Less),
                    Self::ReleaseCandidate(rhs) => lhs.partial_cmp(rhs),
                    Self::Development(_) => Some(Ordering::Less),
                }
            },
            Self::Development(lhs) => {
                // Development builds come after everything else, but are strongly ordered relative to each other
                match other {
                    Self::Development(rhs) => lhs.partial_cmp(rhs),
                    _ => Some(Ordering::Greater),
                }
            }
        }
    }
}

impl GitVersion
{
    pub fn from_parts(release_candidate: Option<usize>, commits: usize, hash: String) -> Self
    {
        Self { release_candidate, commits, hash }
    }
}

impl ToString for GitVersion
{
    fn to_string(&self) -> String
    {
        let base_version = match self.release_candidate {
            None => "".into(),
            Some(rc_number) => format!("-rc{}", rc_number),
        };
        let git_version = format!("-{}-{}", self.commits, self.hash);
        base_version + &git_version
    }
}

impl PartialOrd for GitVersion
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering>
    {
        // Check if this is part of a release candidate-based Git version build
        match self.release_candidate {
            Some(lhs_rc_number) => {
                // It is, so check to see what the other Git version build represents
                match other.release_candidate {
                    // If they're both release candidates, start by checking if they're based on the
                    // same candidate (if they're not, we're done already)
                    Some(rhs_rc_number) => {
                        if lhs_rc_number != rhs_rc_number {
                            return lhs_rc_number.partial_cmp(&rhs_rc_number);
                        }
                    },
                    // Otherwise, if the other is a release, we're already done -
                    // release candidates come before releases
                    None => return Some(Ordering::Less),
                }
            }
            None => {
                // It isi not a release candidate, so check the other to see what that is
                match other.release_candidate {
                    // If thee other is a release candidate, we're done - RC's come before releases
                    Some(_) => return Some(Ordering::Greater),
                    // Otherwise both represent the same base release, continue
                    None => {},
                }
            }
        }

        // If the release candidate logic all passed then we should check how many commits different
        // the two are and if the hashes match
        if self.commits < other.commits {
            Some(Ordering::Less)
        } else if self.commits > other.commits {
            Some(Ordering::Greater)
        } else if self.hash == other.hash {
            Some(Ordering::Equal)
        } else {
            None
        }
    }
}
