// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::sync::{Arc, Mutex};

use color_eyre::eyre::Result;

use crate::serial::bmd_rsp::BmdRspInterface;
use crate::serial::remote::protocol_v3::RemoteV3;
use crate::serial::remote::{BmdAdiV5Protocol, BmdJtagProtocol, BmdRemoteProtocol, BmdSwdProtocol, JtagDev};

pub struct RemoteV4
{
	/// We're a superset of the v3 protocol, this is an instance of that version of the protocol so we
	/// can access the unchanged machinary from it such as the SWD and JTAG low-level protocol components.
	/// This version of the protocol defines new high-level protocol components and support commands only.
	inner_protocol: RemoteV3,
}

impl From<Arc<Mutex<BmdRspInterface>>> for RemoteV4
{
	fn from(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		Self::new(interface)
	}
}

impl RemoteV4
{
	pub(crate) fn new(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		Self {
			inner_protocol: RemoteV3::new(interface),
		}
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
