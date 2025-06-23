// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::sync::{Arc, Mutex};

use bitmask_enum::bitmask;
use color_eyre::eyre::{Report, Result, eyre};
use log::debug;

use crate::serial::bmd_rsp::BmdRspInterface;
use crate::serial::remote::protocol_v3::RemoteV3;
use crate::serial::remote::{
	BmdAdiV5Protocol, BmdJtagProtocol, BmdRemoteProtocol, BmdSwdProtocol, JtagDev, REMOTE_RESP_OK, decode_response,
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
		debug!("Probe supports the following accelerations: {:?}", accelerations);

		Ok(Self {
			inner_protocol: RemoteV3::new(interface),
			accelerations,
		})
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
		None
	}

	fn adiv6_init(&self) -> bool
	{
		false
	}

	fn riscv_jtag_init(&self) -> bool
	{
		false
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
}
