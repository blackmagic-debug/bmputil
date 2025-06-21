// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::sync::{Arc, Mutex, MutexGuard};

use color_eyre::eyre::{Result, eyre};
use log::{debug, warn};

use crate::serial::bmd_rsp::BmdRspInterface;
use crate::serial::remote::{BmdJtagProtocol, BmdRemoteProtocol, BmdSwdProtocol, JtagDev, REMOTE_RESP_ERR};

pub struct RemoteV0
{
	interface: Arc<Mutex<BmdRspInterface>>,
}

pub struct RemoteV0Plus(RemoteV0);

pub struct RemoteV0JTAG
{
	#[allow(unused)]
	interface: Arc<Mutex<BmdRspInterface>>,
}

pub struct RemoteV0SWD
{
	#[allow(unused)]
	interface: Arc<Mutex<BmdRspInterface>>,
}

const REMOTE_SWD_INIT: &str = "!SS#";
const REMOTE_JTAG_INIT: &str = "!JS#";

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

	fn interface(&self) -> MutexGuard<'_, BmdRspInterface>
	{
		self.interface.lock().unwrap()
	}

	pub(crate) fn clone_interface(&self) -> Arc<Mutex<BmdRspInterface>>
	{
		self.interface.clone()
	}
}

impl BmdRemoteProtocol for RemoteV0
{
	fn jtag_init(&self) -> Result<Box<dyn BmdJtagProtocol>>
	{
		// Try to have the probe initialise JTAG comms to any connected targets
		debug!("Remote JTAG init");
		self.interface().buffer_write(REMOTE_JTAG_INIT)?;
		let buffer = self.interface().buffer_read()?;
		// If that failed for some reason, report it and abort
		if buffer.is_empty() || buffer.as_bytes()[0] == REMOTE_RESP_ERR {
			let message = if buffer.len() > 1 {
				&buffer[1..]
			} else {
				"unknown"
			};
			Err(eyre!("Remote JTAG init failed, error {}", message))
		} else {
			// Otherwise, return the v0 JTAG protocol implementation
			Ok(Box::new(RemoteV0JTAG::from(self.clone_interface())))
		}
	}

	fn swd_init(&self) -> Result<Box<dyn BmdSwdProtocol>>
	{
		debug!("Remote SWD init");
		self.interface().buffer_write(REMOTE_SWD_INIT)?;
		let buffer = self.interface().buffer_read()?;
		// If that failed for some reason, report it and abort
		if buffer.is_empty() || buffer.as_bytes()[0] == REMOTE_RESP_ERR {
			let message = if buffer.len() > 1 {
				&buffer[1..]
			} else {
				"unknown"
			};
			Err(eyre!("Remote SWD init failed, error {}", message))
		} else {
			// Otherwise, return the v0 JTAG protocol implementation
			Ok(Box::new(RemoteV0SWD::from(self.clone_interface())))
		}
	}

	fn adiv5_init(&self) -> bool
	{
		warn!("Falling back to non-accelerated probe interface");
		warn!("Please update your probe's firmware for a substantial speed increase");
		false
	}

	fn adiv6_init(&self) -> bool
	{
		warn!("Falling back to non-accelerated probe interface");
		warn!("Please update your probe's firmware for a substantial speed increase");
		false
	}

	fn riscv_jtag_init(&self) -> bool
	{
		warn!("Falling back to non-accelerated probe interface");
		warn!("Please update your probe's firmware for a substantial speed increase");
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

impl From<Arc<Mutex<BmdRspInterface>>> for RemoteV0Plus
{
	fn from(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		warn!(
			"Probe firmware does not support the newer JTAG commands, ADIv6 acceleration or RISC-V JTAG acceleration, \
			 please update it"
		);
		Self(RemoteV0::new(interface))
	}
}

impl BmdRemoteProtocol for RemoteV0Plus
{
	fn jtag_init(&self) -> Result<Box<dyn BmdJtagProtocol>>
	{
		self.0.jtag_init()
	}

	fn swd_init(&self) -> Result<Box<dyn BmdSwdProtocol>>
	{
		self.0.swd_init()
	}

	fn adiv5_init(&self) -> bool
	{
		warn!("Please update your probe's firmware for improved error handling");
		false
	}

	fn adiv6_init(&self) -> bool
	{
		self.0.adiv6_init()
	}

	fn riscv_jtag_init(&self) -> bool
	{
		self.0.riscv_jtag_init()
	}

	fn add_jtag_dev(&self, dev_index: u32, jtag_dev: &JtagDev)
	{
		self.0.add_jtag_dev(dev_index, jtag_dev);
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
}

impl From<Arc<Mutex<BmdRspInterface>>> for RemoteV0JTAG
{
	fn from(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		Self {
			interface,
		}
	}
}

impl BmdJtagProtocol for RemoteV0JTAG
{
	fn tap_reset(&self)
	{
		//
	}

	fn tap_next(&self, _tms: bool, _tdi: bool) -> bool
	{
		false
	}

	fn tap_tms_seq(&self, _tms_states: u32, _clock_cycles: usize)
	{
		//
	}

	fn tap_tdi_tdo_seq(
		&self,
		_data_out: Option<&mut [u8]>,
		_final_tms: bool,
		_data_in: Option<&[u8]>,
		_clock_cycles: usize,
	)
	{
		//
	}

	fn tap_tdi_seq(&self, _final_tms: bool, _data_in: &[u8], _clock_cycles: usize)
	{
		//
	}

	fn tap_cycle(&self, _tms: bool, _tdi: bool, _clock_cycles: usize)
	{
		//
	}
}

impl From<Arc<Mutex<BmdRspInterface>>> for RemoteV0SWD
{
	fn from(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		Self {
			interface,
		}
	}
}

impl BmdSwdProtocol for RemoteV0SWD
{
	fn seq_in(&self, _clock_cycles: usize) -> u32
	{
		0
	}

	fn seq_in_parity(&self, _clock_cycles: usize) -> Option<u32>
	{
		None
	}

	fn seq_out(&self, _value: u32, _clock_cycles: usize)
	{
		//
	}

	fn seq_out_parity(&self, _value: u32, _clock_cycles: usize)
	{
		//
	}
}
