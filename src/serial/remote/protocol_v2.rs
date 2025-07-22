// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::sync::{Arc, Mutex, MutexGuard};

use color_eyre::eyre::{Result, eyre};
use log::{debug, warn};

use crate::serial::bmd_rsp::BmdRspInterface;
use crate::serial::remote::protocol_v0::RemoteV0JTAG;
use crate::serial::remote::protocol_v1::RemoteV1;
use crate::serial::remote::{
	BmdAdiV5Protocol, BmdJtagProtocol, BmdRemoteProtocol, BmdRiscvProtocol, BmdSwdProtocol, JtagDev, REMOTE_RESP_ERR,
	REMOTE_RESP_OK, TargetArchitecture, TargetFamily,
};

pub struct RemoteV2(RemoteV1);

pub struct RemoteV2JTAG(RemoteV0JTAG);

const REMOTE_JTAG_INIT: &str = "!JS#";
/// This command asks the probe if the power is used
const REMOTE_TARGET_VOLTAGE: &str = "!Gp#";

impl From<Arc<Mutex<BmdRspInterface>>> for RemoteV2
{
	fn from(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		warn!("Probe firmware does not support ADIv6 acceleration or RISC-V JTAG acceleration, please update it");
		Self::new(interface)
	}
}

impl RemoteV2
{
	pub(crate) fn new(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		Self(RemoteV1::new(interface))
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

impl BmdRemoteProtocol for RemoteV2
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
			Ok(Box::new(RemoteV2JTAG::from(self.clone_interface())))
		}
	}

	fn swd_init(&self) -> Result<Box<dyn BmdSwdProtocol>>
	{
		self.0.swd_init()
	}

	fn adiv5_init(&self) -> Option<Arc<dyn BmdAdiV5Protocol>>
	{
		self.0.adiv5_init()
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
		self.0.supported_architectures()
	}

	fn supported_families(&self) -> Result<Option<TargetFamily>>
	{
		self.0.supported_families()
	}

	fn get_target_power_state(&self) -> Result<bool>
	{
		self.interface().buffer_write(REMOTE_TARGET_VOLTAGE)?;
		let buffer = self.interface().buffer_read()?;

		if buffer.is_empty() || buffer.as_bytes()[0] != REMOTE_RESP_OK {
			return Err(eyre!("Supported current powered request failed"));
		}

		if buffer.len() < 2 {
			return Err(eyre!("Current powered response is too short"));
		}

		Ok(buffer.as_bytes()[1] == b'1')
	}
}

impl From<Arc<Mutex<BmdRspInterface>>> for RemoteV2JTAG
{
	fn from(interface: Arc<Mutex<BmdRspInterface>>) -> Self
	{
		Self(RemoteV0JTAG::from(interface))
	}
}

/// v2 JTAG enhances v0 JTAG by adding a new command to the set - tap_cycle.
/// This command allows the probe to execute a whole sequence of transitions such as
/// idle cycles without needing each to be an invocation of tap_next, which has significant
/// overhead penalty thanks to USB turnaround times.
impl BmdJtagProtocol for RemoteV2JTAG
{
	fn tap_reset(&self)
	{
		self.0.tap_reset();
	}

	fn tap_next(&self, tms: bool, tdi: bool) -> bool
	{
		self.0.tap_next(tms, tdi)
	}

	fn tap_tms_seq(&self, tms_states: u32, clock_cycles: usize)
	{
		self.0.tap_tms_seq(tms_states, clock_cycles);
	}

	fn tap_tdi_tdo_seq(&self, data_out: Option<&mut [u8]>, final_tms: bool, data_in: Option<&[u8]>, clock_cycles: usize)
	{
		self.0.tap_tdi_tdo_seq(data_out, final_tms, data_in, clock_cycles);
	}

	fn tap_tdi_seq(&self, final_tms: bool, data_in: &[u8], clock_cycles: usize)
	{
		self.0.tap_tdi_seq(final_tms, data_in, clock_cycles);
	}

	fn tap_cycle(&self, _tms: bool, _tdi: bool, _clock_cycles: usize)
	{
		//
	}
}
