use std::mem;
use std::path::PathBuf;

use color_eyre::Report;
use log::{error, warn};
use nusb::{DeviceInfo, list_devices};
use url::Url;

use crate::BmpParams;
use crate::bmp::{BmpDevice, BmpPlatform};
use crate::error::ErrorKind;
use crate::metadata::firmware_download::FirmwareDownload;
use crate::usb::{Pid, PortId, Vid};

#[derive(Debug, Clone, Default)]
pub struct BmpMatcher
{
	index: Option<usize>,
	serial: Option<String>,
	port: Option<PortId>,
}

enum MatchResult
{
	NoMatch(DeviceInfo),
	Found(BmpDevice),
	Error(Report),
}

impl BmpMatcher
{
	pub fn new() -> Self
	{
		Default::default()
	}

	pub fn new_with_port(port: PortId) -> Self
	{
		Self::new().port(Some(port))
	}

	pub fn from_params<Params>(params: &Params) -> Self
	where
		Params: BmpParams,
	{
		Self::new()
			.index(params.index())
			.serial(params.serial_number())
			.port(None)
	}

	/// Set the index to match against.
	#[must_use]
	pub fn index(mut self, idx: Option<usize>) -> Self
	{
		self.index = idx;
		self
	}

	/// Set the serial number to match against.
	#[must_use]
	pub fn serial<'s, IntoOptStrT>(mut self, serial: IntoOptStrT) -> Self
	where
		IntoOptStrT: Into<Option<&'s str>>,
	{
		self.serial = serial.into().map(|s| s.to_string());
		self
	}

	/// Set the port path to match against.
	#[must_use]
	pub fn port(mut self, port: Option<PortId>) -> Self
	{
		self.port = port;
		self
	}

	/// Get any index previously set with `.index()`.
	pub fn get_index(&self) -> Option<usize>
	{
		self.index
	}

	/// Get any serial number previously set with `.serial()`.
	pub fn get_serial(&self) -> Option<&str>
	{
		self.serial.as_deref()
	}

	/// Get any port path previously set with `.port()`.
	pub fn get_port(&self) -> Option<PortId>
	{
		self.port.clone()
	}

	/// Find all connected Black Magic Probe devices that match from the command-line criteria.
	///
	/// This uses the `serial_number`, `index`, and `port` values from `matches`, treating any that
	/// were not provided as always matching.
	///
	/// This function returns all found devices and all errors that occurred during the search.
	/// This is so errors are not hidden, but also do not prevent matching devices from being found.
	/// However, if the length of the error `Vec` is not 0, you should consider the results
	/// potentially incomplete.
	///
	/// The `index` matcher *includes* devices that errored when attempting to match them.
	pub fn find_matching_probes(&self) -> BmpMatchResults
	{
		let mut results = BmpMatchResults {
			found: Vec::new(),
			filtered_out: Vec::new(),
			errors: Vec::new(),
		};

		let devices = match list_devices() {
			Ok(d) => d,
			Err(e) => {
				results.errors.push(e.into());
				return results;
			},
		};

		// Filter out devices that don't match the Black Magic Probe's vid/pid in the first place.
		let devices = devices.filter(|dev| {
			let vid = dev.vendor_id();
			let pid = dev.product_id();
			BmpPlatform::from_vid_pid(Vid(vid), Pid(pid)).is_some()
		});

		devices
			.enumerate()
			.map(|(index, device_info)| self.matching_probe(index, device_info))
			.collect()
	}

	fn matching_probe(&self, index: usize, device_info: DeviceInfo) -> MatchResult
	{
		// Checks if a match is matching an DeviceInfo, and returns the matched state.
		let matched = self.is_probe_matching(index, &device_info);
		if matched {
			match BmpDevice::from_usb_device(device_info) {
				Ok(bmpdev) => MatchResult::Found(bmpdev),
				Err(e) => MatchResult::Error(e),
			}
		} else {
			MatchResult::NoMatch(device_info)
		}
	}

	/// Checks if the serial, index and port matches if specified
	fn is_probe_matching(&self, index: usize, match_information: &impl MatchInformation) -> bool
	{
		// Consider the serial to match if it equals that of the device or if one was not specified at all.
		let serial_matches = self.serial.as_deref().is_none_or(|s| Some(s) == match_information.match_serial_number());

		// Consider the index to match if it equals that of the device or if one was not specified at all.
		let index_matches = self.index.is_none_or(|needle| needle == index);

		// Consider the port to match if it equals that of the device or if one was not specified at all.
		let port_matches = self.port.as_ref().is_none_or(|p| *p == match_information.match_port_id());

		serial_matches && index_matches && port_matches
	}
}

/// Checks if the the information matches with the probe
trait MatchInformation
{
	fn match_serial_number(&self) -> Option<&str>;
	fn match_port_id(&self) -> PortId;
}

impl MatchInformation for DeviceInfo
{
	fn match_serial_number(&self) -> Option<&str>
	{
		self.serial_number()
	}

	fn match_port_id(&self) -> PortId
	{
		PortId::new(self)
	}
}

#[derive(Debug, Default)]
pub struct BmpMatchResults
{
	pub found: Vec<BmpDevice>,
	pub filtered_out: Vec<DeviceInfo>,
	pub errors: Vec<Report>,
}

impl FromIterator<MatchResult> for BmpMatchResults
{
	/// This implements the internals of .collect() on an iterator to convert the iterator MatchResult into a
	/// BmpMatchResults
	fn from_iter<I: IntoIterator<Item = MatchResult>>(iter: I) -> Self
	{
		let mut results = BmpMatchResults {
			found: Vec::new(),
			filtered_out: Vec::new(),
			errors: Vec::new(),
		};

		for match_result in iter {
			match match_result {
				MatchResult::NoMatch(device_info) => results.filtered_out.push(device_info),
				MatchResult::Found(bmpdev) => results.found.push(bmpdev),
				MatchResult::Error(e) => results.errors.push(e),
			};
		}

		results
	}
}

impl BmpMatchResults
{
	/// Pops all found devices, handling printing error and warning cases.
	pub fn pop_all(&mut self) -> color_eyre::Result<Vec<BmpDevice>>
	{
		match self.found.len() {
			0 => {
				// Give some feedback what is found.
				match self.filtered_out.len() {
					0 => {},
					1 => {
						match BmpDevice::from_usb_device(self.filtered_out.pop().expect("The length check makes this a guaranteed assumption")) {
							Ok(bmpdev) => warn!(
								"Matching device not found, but and the following Black Magic Probe device was \
								 filtered out: {}",
								&bmpdev
							),
							Err(_) => {
								warn!("Matching device not found but 1 Black Magic Probe device was filtered out.")
							},
						};
					},
					drained_len => {
						warn!(
							"Matching devices not found but {} Black Magic Probe devices were filtered out.",
							drained_len
						);
						warn!("Filter arguments (--serial, --index, --port) may be incorrect.");
					},
				}
				// Now we're done reporting the filtered, clear it so everything is cleared.
				self.filtered_out.clear();

				if !self.errors.is_empty() {
					warn!("Device not found and errors occurred when searching for devices.");
					warn!(
						"One of these may be why the Black Magic Probe device was not found: {:?}",
						self.errors.as_slice()
					);
				}
				Err(ErrorKind::DeviceNotFound.error().into())
			},
			_ => {
				if !self.errors.is_empty() {
					warn!("Matching device found but errors occurred when searching for devices.");
					warn!("It is unlikely but possible that the incorrect device was selected!");
					warn!("Other device errors: {:?}", self.errors.as_slice());
				}

				Ok(mem::take(&mut self.found))
			},
		}
	}

	/// Pops a single found device, handling printing error and warning cases.
	pub fn pop_single(&mut self, operation: &str) -> color_eyre::Result<BmpDevice, ErrorKind>
	{
		match self.found.len() {
			0 => {
				if !self.filtered_out.is_empty() {
					let (suffix, verb) = if self.filtered_out.len() > 1 {
						("s", "were")
					} else {
						("", "was")
					};
					warn!(
						"Matching device not found and {} Black Magic Probe device{} {} filtered out.",
						self.filtered_out.len(),
						suffix,
						verb,
					);
					warn!("Filter arguments (--serial, --index, --port may be incorrect.");
				}

				if !self.errors.is_empty() {
					warn!("Device not found and errors occurred when searching for devices.");
					warn!(
						"One of these may be why the Black Magic Probe device was not found: {:?}",
						self.errors.as_slice()
					);
				}
				Err(ErrorKind::DeviceNotFound)
			},
			1 => {
				if !self.errors.is_empty() {
					warn!("Matching device found but errors occurred when searching for devices.");
					warn!("It is unlikely but possible that the incorrect device was selected!");
					warn!("Other device errors: {:?}", self.errors.as_slice());
				}

				Ok(self.found.remove(0))
			},
			found_len => {
				error!(
					"{} operation only accepts one Black Magic Probe device, but {} were found!",
					operation, found_len
				);
				error!("Hint: try bmputil info and revise your filter arguments (--serial, --index, --port).");
				Err(ErrorKind::TooManyDevices)
			},
		}
	}

	/// Like `pop_single()`, but does not print helpful diagnostics for edge cases.
	pub(crate) fn pop_single_silent(&mut self) -> color_eyre::Result<BmpDevice, ErrorKind>
	{
		match self.found.len() {
			0 => Err(ErrorKind::DeviceNotFound),
			1 => Ok(self.found.remove(0)),
			_ => Err(ErrorKind::TooManyDevices),
		}
	}
}

#[cfg(test)]
mod tests
{
	use super::*;

	struct TestProbeDevice<'a>
	{
		serial_number: Option<&'a str>,
		port_id: &'a PortId,
	}

	impl MatchInformation for TestProbeDevice<'_>
	{
		fn match_serial_number(&self) -> Option<&str>
		{
			self.serial_number
		}

		fn match_port_id(&self) -> PortId
		{
			self.port_id.clone()
		}
	}

	#[test]
	fn exact_match_success()
	{
		let match_port = &PortId::new_test(1, PathBuf::from("abc"), 2, 3);

		let matching_serial = &String::from("Serial");
		let matcher = BmpMatcher {
			index: Some(1),
			serial: Some(matching_serial.clone()),
			port: Some(match_port.clone()),
		};

		// Match index and probe
		let result = matcher.is_probe_matching(1, &TestProbeDevice {
			serial_number: Some(matching_serial.as_str()),
			port_id: match_port,
		});
		assert_eq!(true, result);
	}

	#[test]
	fn exact_match_failure_index()
	{
		let match_port = &PortId::new_test(1, PathBuf::from("abc"), 2, 3);
		let matching_serial = &String::from("Serial");

		let matcher = BmpMatcher {
			index: Some(1),
			serial: Some(matching_serial.clone()),
			port: Some(match_port.clone()),
		};

		// Don't match on different index
		let result = matcher.is_probe_matching(2, &TestProbeDevice {
			serial_number: Some(matching_serial.as_str()),
			port_id: &match_port.clone(),
		});
		assert_eq!(false, result);
	}

	#[test]
	fn exact_match_failure_port()
	{
		let match_port = &PortId::new_test(1, PathBuf::from("abc"), 2, 3);
		let matching_serial = &String::from("Serial");
		let matcher = BmpMatcher {
			index: Some(1),
			serial: Some(matching_serial.clone()),
			port: Some(match_port.clone()),
		};

		// Don't match on different port
		let not_match_port = PortId::new_test(9, PathBuf::from("xyz"), 8, 7);
		let result = matcher.is_probe_matching(1, &TestProbeDevice {
			serial_number: Some(matching_serial.as_str()),
			port_id: &not_match_port.clone(),
		});
		assert_eq!(false, result);
	}

	#[test]
	fn exact_match_failure_serial_number()
	{
		let match_port = &PortId::new_test(1, PathBuf::from("abc"), 2, 3);
		let matching_serial = &String::from("Serial");

		let matcher = BmpMatcher {
			index: Some(1),
			serial: Some(matching_serial.clone()),
			port: Some(match_port.clone()),
		};

		// Don't match on different serial
		let result = matcher.is_probe_matching(1, &TestProbeDevice {
			serial_number: Some("don't match"),
			port_id: &match_port.clone(),
		});
		assert_eq!(false, result);
	}

	#[test]
	fn match_success_unknown_index()
	{
		let match_port = &PortId::new_test(1, PathBuf::from("abc"), 2, 3);
		let matching_serial = &String::from("Serial");

		let matcher = BmpMatcher {
			index: None,
			serial: Some(matching_serial.clone()),
			port: Some(match_port.clone()),
		};

		let result = matcher.is_probe_matching(1, &TestProbeDevice {
			serial_number: Some(matching_serial),
			port_id: &match_port.clone(),
		});
		assert_eq!(true, result);

		let result = matcher.is_probe_matching(2, &TestProbeDevice {
			serial_number: Some(matching_serial),
			port_id: &match_port.clone(),
		});
		assert_eq!(true, result);
	}

	#[test]
	fn match_success_unknown_serial()
	{
		let match_port = &PortId::new_test(1, PathBuf::from("abc"), 2, 3);

		let matcher = BmpMatcher {
			index: Some(1),
			serial: None,
			port: Some(match_port.clone()),
		};

		let result = matcher.is_probe_matching(1, &TestProbeDevice {
			serial_number: Some(&String::from("Serial")),
			port_id: &match_port.clone(),
		});
		assert_eq!(true, result);

		let result = matcher.is_probe_matching(1, &TestProbeDevice {
			serial_number: Some(&String::from("Unknown")),
			port_id: &match_port.clone(),
		});
		assert_eq!(true, result);
	}

	#[test]
	fn match_success_unknown_port()
	{
		let matching_serial = &String::from("ABC");

		let matcher = BmpMatcher {
			index: Some(1),
			serial: Some(matching_serial.clone()),
			port: None,
		};

		let result = matcher.is_probe_matching(1, &TestProbeDevice {
			serial_number: Some(matching_serial),
			port_id: &PortId::new_test(1, PathBuf::from("abc"), 2, 3),
		});
		assert_eq!(true, result);

		let result = matcher.is_probe_matching(1, &TestProbeDevice {
			serial_number: Some(matching_serial),
			port_id: &PortId::new_test(9, PathBuf::from("xyz"), 8, 7),
		});
		assert_eq!(true, result);
	}
}
