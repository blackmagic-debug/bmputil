// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::sync::{Arc, Mutex, MutexGuard};

use color_eyre::eyre::{Result, eyre};
use log::{debug, warn};

use crate::serial::bmd_rsp::BmdRspInterface;
use crate::serial::remote::adi::{AdiV5AccessPort, AdiV5DebugPort};
use crate::serial::remote::{
	Align, BmdAdiV5Protocol, BmdJtagProtocol, BmdRemoteProtocol, BmdRiscvProtocol, BmdSwdProtocol, JtagDev,
	REMOTE_RESP_ERR, TargetAddr64, TargetArchitecture, TargetFamily,
};

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

pub struct RemoteV0ADIv5
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

	pub(crate) fn interface(&self) -> MutexGuard<'_, BmdRspInterface>
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

	fn adiv5_init(&self) -> Option<Arc<dyn BmdAdiV5Protocol>>
	{
		warn!("Falling back to non-accelerated probe interface");
		warn!("Please update your probe's firmware for a substantial speed increase");
		None
	}

	fn adiv6_init(&self) -> Option<Arc<dyn BmdAdiV5Protocol>>
	{
		warn!("Falling back to non-accelerated probe interface");
		warn!("Please update your probe's firmware for a substantial speed increase");
		None
	}

	fn riscv_jtag_init(&self) -> Option<Arc<dyn BmdRiscvProtocol>>
	{
		warn!("Falling back to non-accelerated probe interface");
		warn!("Please update your probe's firmware for a substantial speed increase");
		None
	}

	/// This is intentionally a no-op on this version of the protocol as the probe has no idea what to do
	/// with the information this would provide. Protocol v1 introduces this machinary
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

	fn supported_architectures(&self) -> Result<Option<TargetArchitecture>>
	{
		Ok(None)
	}

	fn supported_families(&self) -> Result<Option<TargetFamily>>
	{
		Ok(None)
	}

	fn get_target_power_state(&self) -> Result<bool>
	{
		Err(eyre!("Not supported"))
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

impl RemoteV0Plus
{
	pub(crate) fn clone_interface(&self) -> Arc<Mutex<BmdRspInterface>>
	{
		self.0.clone_interface()
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

	fn adiv5_init(&self) -> Option<Arc<dyn BmdAdiV5Protocol>>
	{
		warn!("Please update your probe's firmware for improved error handling");
		Some(Arc::new(RemoteV0ADIv5::from(self.clone_interface())))
	}

	fn adiv6_init(&self) -> Option<Arc<dyn BmdAdiV5Protocol>>
	{
		self.0.adiv6_init()
	}

	fn riscv_jtag_init(&self) -> Option<Arc<dyn BmdRiscvProtocol>>
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

impl From<Arc<Mutex<BmdRspInterface>>> for RemoteV0ADIv5
{
	fn from(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		Self {
			interface,
		}
	}
}

impl BmdAdiV5Protocol for RemoteV0ADIv5
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
