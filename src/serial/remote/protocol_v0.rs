// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::sync::{Arc, Mutex};

use color_eyre::eyre::{Result, eyre};
use log::warn;

use crate::serial::bmd_rsp::BmdRspInterface;
use crate::serial::remote::{BmdJtagProtocol, BmdRemoteProtocol, BmdSwdProtocol, JtagDev};

pub struct RemoteV0
{
	#[allow(unused)]
	interface: Arc<Mutex<BmdRspInterface>>,
}

impl From<Arc<Mutex<BmdRspInterface>>> for RemoteV0
{
	fn from(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		warn!(
			"Probe firmware does not support the newer JTAG commands, ADIv5 acceleration, ADIv6 acceleration or \
			 RISC-V JTAG acceleration, please update it"
		);
		Self::new(interface)
	}
}

impl RemoteV0
{
	pub(crate) fn new(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		Self {
			interface,
		}
	}
}

impl BmdRemoteProtocol for RemoteV0
{
	fn jtag_init(&self) -> Result<Box<dyn BmdJtagProtocol>>
	{
		Err(eyre!(""))
	}

	fn swd_init(&self) -> Result<Box<dyn BmdSwdProtocol>>
	{
		Err(eyre!(""))
	}

	fn adiv5_init(&self) -> bool
	{
		false
	}

	fn adiv6_init(&self) -> bool
	{
		false
	}

	fn riscv_jtag_init(&self) -> bool
	{
		false
	}

	fn add_jtag_dev(&self, _dev_index: u32, _jtag_dev: &JtagDev) {}

	fn get_comms_frequency(&self) -> u32
	{
		u32::MAX
	}

	fn set_comms_frequency(&self, _freq: u32) -> bool
	{
		false
	}

	fn target_clk_output_enable(&self, _enable: bool)
	{
		//
	}
}
