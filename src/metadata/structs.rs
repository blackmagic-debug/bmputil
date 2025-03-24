use std::{collections::BTreeMap, path::PathBuf};
use std::str::FromStr;
use std::string::ToString;

use reqwest::Url;
use serde::Deserialize;
use serde::de::Visitor;

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

#[derive(PartialEq, PartialOrd, Eq, Ord, Clone, Copy)]
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

struct ProbeVisitor;

#[derive(Deserialize)]
pub struct Firmware
{
	#[serde(flatten)]
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

impl ToString for Probes
{
	fn to_string(&self) -> String
	{
		match self {
			Probes::_96bCarbon => "96b_carbon",
			Probes::BlackpillF401CC => "blackpill-f401cc",
			Probes::BlackpillF401CE => "blackpill-f401ce",
			Probes::BlackpillF411CE => "blackpill-f411ce",
			Probes::Bluepill => "bluepill",
			Probes::CtxLink => "ctxlink",
			Probes::F072 => "f072",
			Probes::F3 => "f3",
			Probes::F4Discovery => "f4discovery",
			Probes::Hydrabus => "hydrabus",
			Probes::LaunchpadICDI => "launchpad-icdi",
			Probes::Native => "native",
			Probes::Stlink => "stlink",
			Probes::Stlinkv3 => "stlinkv3",
			Probes::Swlink => "swlink",
		}.to_string()
	}
}

impl<'de> Deserialize<'de> for Probes
{
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
		where D: serde::Deserializer<'de>
	{
		deserializer.deserialize_str(ProbeVisitor)
	}
}

impl<'de> Visitor<'de> for ProbeVisitor
{
	type Value = Probes;

	fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result
	{
		formatter.write_str("a valid probe platform name")
	}

	fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
		where E: serde::de::Error,
	{
		Probes::from_str(value)
			.map_err(|e| E::custom(e.to_string()))
	}
}
