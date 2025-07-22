// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::fmt::Display;
use std::sync::{Arc, Mutex, MutexGuard};

use bitmask_enum::bitmask;
use color_eyre::eyre::{Report, Result, eyre};
use log::{debug, warn};

use crate::serial::bmd_rsp::BmdRspInterface;
use crate::serial::remote::adi::{AdiV5AccessPort, AdiV5DebugPort};
use crate::serial::remote::protocol_v3::RemoteV3;
use crate::serial::remote::riscv_debug::RiscvDmi;
use crate::serial::remote::{
	Align, BmdAdiV5Protocol, BmdJtagProtocol, BmdRemoteProtocol, BmdRiscvProtocol, BmdSwdProtocol, JtagDev,
	REMOTE_RESP_NOTSUP, REMOTE_RESP_OK, TargetAddr64, TargetArchitecture, TargetFamily, decode_response,
};

pub struct RemoteV4
{
	/// We're a superset of the v3 protocol, this is an instance of that version of the protocol so we
	/// can access the unchanged machinary from it such as the SWD and JTAG low-level protocol components.
	/// This version of the protocol defines new high-level protocol components and support commands only.
	inner_protocol: RemoteV3,
	/// Bitmask of the accelerations supported by this probe
	#[allow(unused)]
	accelerations: Acceleration,
}

pub struct RemoteV4ADIv5
{
	#[allow(unused)]
	interface: Arc<Mutex<BmdRspInterface>>,
}

pub struct RemoteV4ADIv6
{
	#[allow(unused)]
	interface: Arc<Mutex<BmdRspInterface>>,
}

pub struct RemoteV4RiscvJtag
{
	#[allow(unused)]
	interface: Arc<Mutex<BmdRspInterface>>,
}

#[bitmask(u64)]
#[bitmask_config(vec_debug)]
enum Acceleration
{
	ADIv5,
	CortexAR,
	RiscV,
	ADIv6,
}

/// This command asks the probe what high-level protocol accelerations it supports
const REMOTE_HL_ACCEL: &str = "!HA#";
/// This command asks the probe what target architectures the firmware build supports
const REMOTE_HL_ARCHS: &str = "!Ha#";
/// This command asks the probe what target families the firmware build supports
const REMOTE_HL_FAMILIES: &str = "!HF#";

impl TryFrom<Arc<Mutex<BmdRspInterface>>> for RemoteV4
{
	type Error = Report;

	fn try_from(interface: Arc<Mutex<BmdRspInterface>>) -> Result<Self>
	{
		Self::new(interface)
	}
}

impl RemoteV4
{
	pub(crate) fn new(interface: Arc<Mutex<BmdRspInterface>>) -> Result<Self>
	{
		// Before we can create an instance of the remote protocol structure, we first need to ask
		// the probe about supported accelerations as this determines the results of asking for the
		// high-level accelerations below. Start by firing off the request to the probe
		let mut iface = interface.lock().unwrap();
		iface.buffer_write(REMOTE_HL_ACCEL)?;
		// Read back the result and relinquish our comms lock so structure creation can work
		let buffer = iface.buffer_read()?;
		drop(iface);
		// Check for communication failures
		if buffer.is_empty() || buffer.as_bytes()[0] != REMOTE_RESP_OK {
			return Err(eyre!(
				"Error talking with probe, expected OK response to supported accelerations query, got {:?}",
				buffer
			));
		}
		// Decode the response and translate the supported accelerations bitmask to our internal
		// enumeration of accelerations
		let accelerations = Acceleration::from(decode_response(&buffer[1..], 8));
		debug!("Probe supports the following accelerations: {}", accelerations);

		Ok(Self {
			inner_protocol: RemoteV3::new(interface),
			accelerations,
		})
	}

	pub(crate) fn interface(&self) -> MutexGuard<'_, BmdRspInterface>
	{
		self.inner_protocol.interface()
	}

	pub(crate) fn clone_interface(&self) -> Arc<Mutex<BmdRspInterface>>
	{
		self.inner_protocol.clone_interface()
	}
}

impl BmdRemoteProtocol for RemoteV4
{
	fn jtag_init(&self) -> Result<Box<dyn BmdJtagProtocol>>
	{
		self.inner_protocol.jtag_init()
	}

	fn swd_init(&self) -> Result<Box<dyn BmdSwdProtocol>>
	{
		self.inner_protocol.swd_init()
	}

	fn adiv5_init(&self) -> Option<Arc<dyn BmdAdiV5Protocol>>
	{
		if self.accelerations.contains(Acceleration::ADIv5) {
			Some(Arc::new(RemoteV4ADIv5::from(self.clone_interface())))
		} else {
			None
		}
	}

	fn adiv6_init(&self) -> Option<Arc<dyn BmdAdiV5Protocol>>
	{
		if self.accelerations.contains(Acceleration::ADIv6) {
			Some(Arc::new(RemoteV4ADIv6::from(self.clone_interface())))
		} else {
			None
		}
	}

	fn riscv_jtag_init(&self) -> Option<Arc<dyn BmdRiscvProtocol>>
	{
		if self.accelerations.contains(Acceleration::RiscV) {
			Some(Arc::new(RemoteV4RiscvJtag::from(self.clone_interface())))
		} else {
			None
		}
	}

	fn add_jtag_dev(&self, dev_index: u32, jtag_dev: &JtagDev)
	{
		self.inner_protocol.add_jtag_dev(dev_index, jtag_dev);
	}

	fn get_comms_frequency(&self) -> u32
	{
		self.inner_protocol.get_comms_frequency()
	}

	fn set_comms_frequency(&self, freq: u32) -> bool
	{
		self.inner_protocol.set_comms_frequency(freq)
	}

	fn target_clk_output_enable(&self, enable: bool)
	{
		self.inner_protocol.target_clk_output_enable(enable);
	}

	fn supported_architectures(&self) -> Result<Option<TargetArchitecture>>
	{
		// Send the request to the probe
		self.interface().buffer_write(REMOTE_HL_ARCHS)?;
		let buffer = self.interface().buffer_read()?;
		// Check too see if that failed for some reason
		if buffer.is_empty() || (buffer.as_bytes()[0] != REMOTE_RESP_OK && buffer.as_bytes()[0] != REMOTE_RESP_NOTSUP) {
			let message = if buffer.len() > 1 {
				&buffer[1..]
			} else {
				"unknown"
			};
			Err(eyre!("Supported architectures request failed, error {}", message))
		} else if buffer.as_bytes()[0] == REMOTE_RESP_NOTSUP {
			// If we get here, the probe talks v4 but doesn't know this command - meaning pre-v2.0.0 firmware
			// but post-v1.10.2. Ask the user to upgrade off development firmware onto the release or later.
			warn!("Please upgrade your firmware to allow checking supported target architectures to work properly");
			Ok(None)
		} else {
			// We got a good response, decode it and turn the value into a bitfield return
			let architectures = decode_response(&buffer[1..], 8);
			Ok(Some(architectures.into()))
		}
	}

	fn supported_families(&self) -> Result<Option<TargetFamily>>
	{
		// Send the request to the probe
		self.interface().buffer_write(REMOTE_HL_FAMILIES)?;
		let buffer = self.interface().buffer_read()?;
		// Check too see if that failed for some reason
		if buffer.is_empty() || (buffer.as_bytes()[0] != REMOTE_RESP_OK && buffer.as_bytes()[0] != REMOTE_RESP_NOTSUP) {
			let message = if buffer.len() > 1 {
				&buffer[1..]
			} else {
				"unknown"
			};
			Err(eyre!("Supported architectures request failed, error {}", message))
		} else if buffer.as_bytes()[0] == REMOTE_RESP_NOTSUP {
			// If we get here, the probe talks v4 but doesn't know this command - meaning pre-v2.0.0 firmware
			// but post-v1.10.2. Ask the user to upgrade off development firmware onto the release or later.
			warn!("Please upgrade your firmware to allow checking supported target families to work properly");
			Ok(None)
		} else {
			// We got a good response, decode it and turn the value into a bitfield return
			let families = decode_response(&buffer[1..], 8);
			Ok(Some(families.into()))
		}
	}

	fn get_target_power_state(&self) -> Result<bool>
	{
		self.inner_protocol.get_target_power_state()
	}
}

impl From<Arc<Mutex<BmdRspInterface>>> for RemoteV4ADIv5
{
	fn from(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		Self {
			interface,
		}
	}
}

impl BmdAdiV5Protocol for RemoteV4ADIv5
{
	fn raw_access(&self, _dp: AdiV5DebugPort, _rnw: u8, _addr: u16, _value: u32) -> u32
	{
		0
	}

	fn dp_read(&self, _dp: AdiV5DebugPort, _addr: u16) -> u32
	{
		0
	}

	fn ap_read(&self, _ap: AdiV5AccessPort, _addr: u16) -> u32
	{
		0
	}

	fn ap_write(&self, _ap: AdiV5AccessPort, _addr: u16, _value: u32)
	{
		//
	}

	fn mem_read(&self, _ap: AdiV5AccessPort, _dest: &mut [u8], _src: TargetAddr64)
	{
		//
	}

	fn mem_write(&self, _ap: AdiV5AccessPort, _dest: TargetAddr64, _src: &[u8], _align: Align)
	{
		//
	}
}

impl From<Arc<Mutex<BmdRspInterface>>> for RemoteV4ADIv6
{
	fn from(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		Self {
			interface,
		}
	}
}

impl BmdAdiV5Protocol for RemoteV4ADIv6
{
	fn raw_access(&self, _dp: AdiV5DebugPort, _rnw: u8, _addr: u16, _value: u32) -> u32
	{
		0
	}

	fn dp_read(&self, _dp: AdiV5DebugPort, _addr: u16) -> u32
	{
		0
	}

	fn ap_read(&self, _ap: AdiV5AccessPort, _addr: u16) -> u32
	{
		0
	}

	fn ap_write(&self, _ap: AdiV5AccessPort, _addr: u16, _value: u32)
	{
		//
	}

	fn mem_read(&self, _ap: AdiV5AccessPort, _dest: &mut [u8], _src: TargetAddr64)
	{
		//
	}

	fn mem_write(&self, _ap: AdiV5AccessPort, _dest: TargetAddr64, _src: &[u8], _align: Align)
	{
		//
	}
}

impl From<Arc<Mutex<BmdRspInterface>>> for RemoteV4RiscvJtag
{
	fn from(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		Self {
			interface,
		}
	}
}

impl BmdRiscvProtocol for RemoteV4RiscvJtag
{
	fn dmi_read(&self, _dmi: RiscvDmi, _address: u32) -> Option<u32>
	{
		None
	}

	fn dmi_write(&self, _dmi: RiscvDmi, _address: u32, _value: u32) -> bool
	{
		false
	}
}

impl Display for Acceleration
{
	fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
	{
		let mut accelerations = Vec::with_capacity(4);
		if self.contains(Self::ADIv5) {
			accelerations.push("ADIv5");
		}
		if self.contains(Self::ADIv6) {
			accelerations.push("ADIv6");
		}
		if self.contains(Self::RiscV) {
			accelerations.push("RISC-V");
		}
		if self.contains(Self::CortexAR) {
			accelerations.push("Cortex-A/R");
		}
		write!(fmt, "{}", accelerations.join(", "))
	}
}
