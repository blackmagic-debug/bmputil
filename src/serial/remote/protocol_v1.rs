// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::sync::{Arc, Mutex, MutexGuard};

use color_eyre::eyre::Result;
use log::warn;

use crate::serial::bmd_rsp::BmdRspInterface;
use crate::serial::remote::adi::{AdiV5AccessPort, AdiV5DebugPort};
use crate::serial::remote::protocol_v0::RemoteV0;
use crate::serial::remote::{
	Align, BmdAdiV5Protocol, BmdJtagProtocol, BmdRemoteProtocol, BmdRiscvProtocol, BmdSwdProtocol, JtagDev,
	TargetAddr64, TargetArchitecture, TargetFamily,
};

pub struct RemoteV1(RemoteV0);

pub struct RemoteV1ADIv5
{
	#[allow(unused)]
	interface: Arc<Mutex<BmdRspInterface>>,
}

impl From<Arc<Mutex<BmdRspInterface>>> for RemoteV1
{
	fn from(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		warn!(
			"Probe firmware does not support the newer JTAG commands, ADIv6 acceleration or RISC-V JTAG acceleration, \
			 please update it"
		);
		Self::new(interface)
	}
}

impl RemoteV1
{
	pub(crate) fn new(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		Self(RemoteV0::new(interface))
	}

	pub(crate) fn interface(&self) -> MutexGuard<BmdRspInterface>
	{
		self.0.interface()
	}

	pub(crate) fn clone_interface(&self) -> Arc<Mutex<BmdRspInterface>>
	{
		self.0.clone_interface()
	}
}

impl BmdRemoteProtocol for RemoteV1
{
	fn jtag_init(&self) -> Result<Box<dyn BmdJtagProtocol>>
	{
		self.0.jtag_init()
	}

	fn swd_init(&self) -> Result<Box<dyn BmdSwdProtocol>>
	{
		self.0.swd_init()
	}

	fn adiv5_init(&self) -> Option<Arc<dyn BmdAdiV5Protocol>>
	{
		warn!("Please update your probe's firmware for improved error handling");
		Some(Arc::new(RemoteV1ADIv5::from(self.clone_interface())))
	}

	fn adiv6_init(&self) -> Option<Arc<dyn BmdAdiV5Protocol>>
	{
		self.0.adiv6_init()
	}

	fn riscv_jtag_init(&self) -> Option<Arc<dyn BmdRiscvProtocol>>
	{
		self.0.riscv_jtag_init()
	}

	fn add_jtag_dev(&self, _dev_index: u32, _jtag_dev: &JtagDev)
	{
		//
	}

	fn get_comms_frequency(&self) -> u32
	{
		self.0.get_comms_frequency()
	}

	fn set_comms_frequency(&self, freq: u32) -> bool
	{
		self.0.set_comms_frequency(freq)
	}

	fn target_clk_output_enable(&self, enable: bool)
	{
		self.0.target_clk_output_enable(enable);
	}

	fn supported_architectures(&self) -> Result<Option<TargetArchitecture>>
	{
		self.0.supported_architectures()
	}

	fn supported_families(&self) -> Result<Option<TargetFamily>>
	{
		self.0.supported_families()
	}

	fn get_target_power_state(&self) -> Result<bool>
	{
		self.0.get_target_power_state()
	}
}

impl From<Arc<Mutex<BmdRspInterface>>> for RemoteV1ADIv5
{
	fn from(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		Self {
			interface,
		}
	}
}

impl BmdAdiV5Protocol for RemoteV1ADIv5
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

	fn ap_write(&self, _ap: AdiV5AccessPort, _addr: u16, _value: u32) {}

	fn mem_read(&self, _ap: AdiV5AccessPort, _dest: &mut [u8], _src: TargetAddr64)
	{
		//
	}

	fn mem_write(&self, _ap: AdiV5AccessPort, _dest: TargetAddr64, _src: &[u8], _align: Align)
	{
		//
	}
}
