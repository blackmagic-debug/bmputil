use std::{collections::BTreeMap, path::PathBuf};
use std::str::FromStr;

use reqwest::Url;
use serde::Deserialize;

use crate::error::{Error, ErrorKind};

#[derive(Deserialize)]
pub struct Metadata
{
	pub version: usize,
	pub releases: BTreeMap<String, Release>,
}

#[derive(Deserialize)]
pub struct Release
{
	#[serde(rename = "includesBMDA")]
	pub includes_bmda: bool,
	pub firmware: BTreeMap<Probes, Firmware>,
}

#[derive(PartialEq, PartialOrd, Eq, Ord, Deserialize)]
pub enum Probes
{
	_96bCarbon,
	BlackpillF401CC,
	BlackpillF401CE,
	BlackpillF411CE,
	Bluepill,
	CtxLink,
	F072,
	F3,
	F4Discovery,
	Hydrabus,
	LaunchpadICDI,
	Native,
	Stlink,
	Stlinkv3,
	Swlink,
}

#[derive(Deserialize)]
pub struct Firmware
{
	pub variants: BTreeMap<String, FirmwareDownload>
}

#[derive(Deserialize)]
pub struct FirmwareDownload
{
	#[serde(rename = "friendlyName")]
	pub friendly_name: String,
	#[serde(rename = "fileName")]
	pub file_name: PathBuf,
	pub uri: Url,
}

impl FromStr for Probes
{
	type Err = Error;

	fn from_str(value: &str) -> Result<Self, Self::Err>
	{
		match value {
			"96b_carbon" => Ok(Probes::_96bCarbon),
			"blackpill-f401cc" => Ok(Probes::BlackpillF401CC),
			"blackpill-f401ce" => Ok(Probes::BlackpillF401CE),
			"blackpill-f411ce" => Ok(Probes::BlackpillF411CE),
			"bluepill" => Ok(Probes::Bluepill),
			"ctxlink" => Ok(Probes::CtxLink),
			"f072" => Ok(Probes::F072),
			"f3" => Ok(Probes::F3),
			"f4discovery" => Ok(Probes::F4Discovery),
			"hydrabus" => Ok(Probes::Hydrabus),
			"launchpad-icdi" => Ok(Probes::LaunchpadICDI),
			"native" => Ok(Probes::Native),
			"stlink" => Ok(Probes::Stlink),
			"stlinkv3" => Ok(Probes::Stlinkv3),
			"swlink" => Ok(Probes::Swlink),
			&_ => Err(Error::new(ErrorKind::ReleaseMetadataInvalid, None))
		}
	}
}
