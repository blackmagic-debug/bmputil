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
	pub firmware: BTreeMap<Probe, Firmware>,
}

#[derive(PartialEq, PartialOrd, Eq, Ord, Clone, Copy)]
pub enum Probe
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

impl FromStr for Probe
{
	type Err = Error;

	fn from_str(value: &str) -> Result<Self, Self::Err>
	{
		match value {
			"96b_carbon" => Ok(Probe::_96bCarbon),
			"blackpill-f401cc" => Ok(Probe::BlackpillF401CC),
			"blackpill-f401ce" => Ok(Probe::BlackpillF401CE),
			"blackpill-f411ce" => Ok(Probe::BlackpillF411CE),
			"bluepill" => Ok(Probe::Bluepill),
			"ctxlink" => Ok(Probe::CtxLink),
			"f072" => Ok(Probe::F072),
			"f3" => Ok(Probe::F3),
			"f4discovery" => Ok(Probe::F4Discovery),
			"hydrabus" => Ok(Probe::Hydrabus),
			"launchpad-icdi" => Ok(Probe::LaunchpadICDI),
			"native" => Ok(Probe::Native),
			"stlink" => Ok(Probe::Stlink),
			"stlinkv3" => Ok(Probe::Stlinkv3),
			"swlink" => Ok(Probe::Swlink),
			&_ => Err(Error::new(ErrorKind::ReleaseMetadataInvalid, None))
		}
	}
}

impl ToString for Probe
{
	fn to_string(&self) -> String
	{
		match self {
			Probe::_96bCarbon => "96b_carbon",
			Probe::BlackpillF401CC => "blackpill-f401cc",
			Probe::BlackpillF401CE => "blackpill-f401ce",
			Probe::BlackpillF411CE => "blackpill-f411ce",
			Probe::Bluepill => "bluepill",
			Probe::CtxLink => "ctxlink",
			Probe::F072 => "f072",
			Probe::F3 => "f3",
			Probe::F4Discovery => "f4discovery",
			Probe::Hydrabus => "hydrabus",
			Probe::LaunchpadICDI => "launchpad-icdi",
			Probe::Native => "native",
			Probe::Stlink => "stlink",
			Probe::Stlinkv3 => "stlinkv3",
			Probe::Swlink => "swlink",
		}.to_string()
	}
}

impl<'de> Deserialize<'de> for Probe
{
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
		where D: serde::Deserializer<'de>
	{
		deserializer.deserialize_str(ProbeVisitor)
	}
}

impl<'de> Visitor<'de> for ProbeVisitor
{
	type Value = Probe;

	fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result
	{
		formatter.write_str("a valid probe platform name")
	}

	fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
		where E: serde::de::Error,
	{
		Probe::from_str(value)
			.map_err(|e| E::custom(e.to_string()))
	}
}
