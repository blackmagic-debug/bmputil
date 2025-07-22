use std::mem;

use color_eyre::Report;
use log::{error, warn};
use nusb::{DeviceInfo, list_devices};

use crate::BmpParams;
use crate::bmp::{BmpDevice, BmpPlatform};
use crate::error::ErrorKind;
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

	/// Checks if a match is matching an DeviceInfo, and returns the matched state.
	fn matching_probe(&self, index: usize, device_info: DeviceInfo) -> MatchResult
	{
		// Consider the serial to match if it equals that of the device or if one was not specified at all.
		let serial_matches = self
			.serial
			.as_deref()
			.is_none_or(|s| Some(s) == device_info.serial_number());

		// Consider the index to match if it equals that of the device or if one was not specified at all.
		let index_matches = self.index.is_none_or(|needle| needle == index);

		// Consider the port to match if it equals that of the device or if one was not specified at all.
		let port_matches = self.port.as_ref().is_none_or(|p| {
			let port = PortId::new(&device_info);

			p == &port
		});

		// Finally, check the provided matchers.
		if index_matches && port_matches && serial_matches {
			match BmpDevice::from_usb_device(device_info) {
				Ok(bmpdev) => MatchResult::Found(bmpdev),
				Err(e) => MatchResult::Error(e),
			}
		} else {
			MatchResult::NoMatch(device_info)
		}
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
						match BmpDevice::from_usb_device(
							self.filtered_out
								.pop()
								.expect("The length check makes this a guaranteed assumption"),
						) {
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
