// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::fmt::Display;
use std::sync::{Arc, Mutex};

use bitmask_enum::bitmask;
use color_eyre::eyre::Result;

use crate::serial::bmd_rsp::BmdRspInterface;
use crate::serial::remote::adi::{AdiV5AccessPort, AdiV5DebugPort};
use crate::serial::remote::protocol_v0::{RemoteV0, RemoteV0Plus};
use crate::serial::remote::protocol_v1::RemoteV1;
use crate::serial::remote::protocol_v2::RemoteV2;
use crate::serial::remote::protocol_v3::RemoteV3;
use crate::serial::remote::protocol_v4::RemoteV4;
use crate::serial::remote::riscv_debug::RiscvDmi;

pub mod adi;
mod protocol_v0;
mod protocol_v1;
mod protocol_v2;
mod protocol_v3;
mod protocol_v4;
pub mod riscv_debug;

/// This is the max possible size of a remote protocol packet which a hard limitation of the
/// firmware on the probe - 1KiB is all the buffer that could be spared.
pub const REMOTE_MAX_MSG_SIZE: usize = 1024;

/// Start of message marker for the protocol
pub const REMOTE_SOM: u8 = b'!';
/// End of message marker for the protocol
pub const REMOTE_EOM: u8 = b'#';
/// Response marker for the protocol
pub const REMOTE_RESP: u8 = b'&';

/// Probe response was okay and the data returned is valid
pub const REMOTE_RESP_OK: u8 = b'K';
/// Probe found an error with a request parameter
pub const REMOTE_RESP_PARERR: u8 = b'P';
/// Probe encountered an error executing the request
pub const REMOTE_RESP_ERR: u8 = b'E';
/// Probe does not support the request made
pub const REMOTE_RESP_NOTSUP: u8 = b'N';

pub type TargetAddr32 = u32;
pub type TargetAddr64 = u64;

/// Alignments available for use by memory accesses
#[repr(u8)]
pub enum Align
{
	As8Bit,
	As16Bit,
	As32Bit,
	As64Bit,
}

#[bitmask(u64)]
pub enum TargetArchitecture
{
	CortexM,
	CortexAR,
	RiscV32,
	RiscV64,
}

#[bitmask(u64)]
pub enum TargetFamily
{
	AT32,
	Apollo3,
	CH32,
	CH579,
	EFM,
	GD32,
	HC32,
	LPC,
	MM32,
	NRF,
	NXPKinetis,
	Puya,
	RenesasRZ,
	RenesasRA,
	RP,
	SAM,
	STM,
	TI,
	Xilinx,
	NXPiMXRT,
}

/// Types implementing this trait implement the common portion of the BMD remote protocol
/// (this includes things like comms initialisation, and clock frequency control)
pub trait BmdRemoteProtocol
{
	// Comms protocol initialisation functions
	fn swd_init(&self) -> Result<Box<dyn BmdSwdProtocol>>;
	fn jtag_init(&self) -> Result<Box<dyn BmdJtagProtocol>>;
	// Higher level protocol initialisation functions
	fn adiv5_init(&self) -> Option<Arc<dyn BmdAdiV5Protocol>>;
	fn adiv6_init(&self) -> Option<Arc<dyn BmdAdiV5Protocol>>;
	fn riscv_jtag_init(&self) -> Option<Arc<dyn BmdRiscvProtocol>>;

	// Probe operation control functions
	fn add_jtag_dev(&self, dev_index: u32, jtag_dev: &JtagDev);
	fn get_comms_frequency(&self) -> u32;
	fn set_comms_frequency(&self, freq: u32) -> bool;
	fn target_clk_output_enable(&self, enable: bool);
	fn supported_architectures(&self) -> Result<Option<TargetArchitecture>>;
	fn supported_families(&self) -> Result<Option<TargetFamily>>;
	fn get_target_power_state(&self) -> Result<bool>;
}

/// Types implementing this trait provide raw SWD access to targets over the BMD remote protocol
pub trait BmdSwdProtocol
{
	/// Executes a read of the SWD bus for `clock_cycles` clock cycles, for up to 32 cycles,
	/// and returns the result as a 32-bit integer
	fn seq_in(&self, clock_cycles: usize) -> u32;
	/// The same as seq_in but then does one additional cycle to read a parity bit, checks
	/// the parity bit's value, and then only returns the result if the parity check passes -
	/// returns None otherwise
	fn seq_in_parity(&self, clock_cycles: usize) -> Option<u32>;
	/// Executes a write to the SWD bus for `clock_cycles` clock cycles, for up to 32 cycles,
	/// putting out the value provided to the bus
	fn seq_out(&self, value: u32, clock_cycles: usize);
	/// The same as seq_out but then computes the parity bit for the provided value, and
	/// does one additional cycle to write that parity bit out to thebus
	fn seq_out_parity(&self, value: u32, clock_cycles: usize);
}

/// Types implementing this trait provide raw JTAG access to targets over the BMD remote protocol
pub trait BmdJtagProtocol
{
	// Note: signal names are as for the device under test.

	/// Executes a state machine reset to ensure a clean, known TAP state
	fn tap_reset(&self);
	/// Executes one state transition in the JTAG TAP state machine:
	/// - Ensure TCK is low
	/// - Assert the values of TMS and TDI
	/// - Assert TCK (TMS and TDO are latched on rising edge)
	/// - Capture the value of TDO
	/// - Release TCK
	fn tap_next(&self, tms: bool, tdi: bool) -> bool;
	/// Performs a sequence of cycles with the provided bitstring of TMS states
	fn tap_tms_seq(&self, tms_states: u32, clock_cycles: usize);
	/// Shift out a sequence on TDI, capture data from TDO. Holds TMS low till the final cycle,
	/// then uses the value of final_tms to determine what state to put TMS into.
	/// - This is not endian safe: The first byte will always be shifted out first.
	/// - The TDO buffer may be given as None to ignore captured data.
	/// - The TDI buffer may be given as None to only capture result data (if no data is given, dummy data will be
	///   synthesised for the request cycle)
	fn tap_tdi_tdo_seq(
		&self,
		data_out: Option<&mut [u8]>,
		final_tms: bool,
		data_in: Option<&[u8]>,
		clock_cycles: usize,
	);
	/// Shift out a sequence on TDI. Holds TMS low till the final cycle, then uses the value
	/// of final_tms to determine what state to put tMS into.
	/// - This is not endian safe: The first byte will always be shifted out first.
	fn tap_tdi_seq(&self, final_tms: bool, data_in: &[u8], clock_cycles: usize);
	/// Perform a series of cycles on the state machine with TMS and TDI held in a set state
	fn tap_cycle(&self, tms: bool, tdi: bool, clock_cycles: usize);
}

/// Types implementing this trait provide accelerated ADIv5 access to targets over the BMD remote protocol
pub trait BmdAdiV5Protocol
{
	/// Perform a raw AP or DP register access against the target, reporting the read result back
	fn raw_access(&self, dp: AdiV5DebugPort, rnw: u8, addr: u16, value: u32) -> u32;
	/// Read a DP (or AP*) register from the target
	fn dp_read(&self, dp: AdiV5DebugPort, addr: u16) -> u32;
	/// Read an AP register from the target
	fn ap_read(&self, ap: AdiV5AccessPort, addr: u16) -> u32;
	/// Write an AP register on the target
	fn ap_write(&self, ap: AdiV5AccessPort, addr: u16, value: u32);
	/// Read memory associated with an AP from the target into the buffer passed to dest
	fn mem_read(&self, ap: AdiV5AccessPort, dest: &mut [u8], src: TargetAddr64);
	/// Write memory associated with an AP to the target from the buffer passed in src and with the
	/// access alignment given by align
	fn mem_write(&self, ap: AdiV5AccessPort, dest: TargetAddr64, src: &[u8], align: Align);
}

pub trait BmdRiscvProtocol
{
	fn dmi_read(&self, dmi: RiscvDmi, address: u32) -> Option<u32>;
	fn dmi_write(&self, dmi: RiscvDmi, address: u32, value: u32) -> bool;
}

/// Structure representing a device on the JTAG scan chain
#[allow(unused)]
pub struct JtagDev
{
	idcode: u32,
	current_ir: u32,

	dr_prescan: u8,
	dr_postscan: u8,

	ir_len: u8,
	ir_prescan: u8,
	ir_postscan: u8,
}

#[derive(Copy, Clone)]
pub(crate) enum ProtocolVersion
{
	Unknown,
	V0,
	V0Plus,
	V1,
	V2,
	V3,
	V4,
}

pub fn decode_response(response: &str, digits: usize) -> u64
{
	// Clamp the number of digits to the number actually available
	let digits = if digits > response.len() {
		response.len()
	} else {
		digits
	};

	let mut value = 0;
	// For each byte in the response that we care about, un-hexify the byte
	for byte in response[..digits].chars() {
		value <<= 4;
		value |= byte.to_digit(16).unwrap() as u64;
	}

	value
}

impl ProtocolVersion
{
	/// Extract an instance of the BMD remote protocol communication object for this version of the protocol
	pub fn protocol_impl(&self, interface: Arc<Mutex<BmdRspInterface>>) -> Result<Box<dyn BmdRemoteProtocol>>
	{
		match self {
			Self::V0 => Ok(Box::new(RemoteV0::from(interface))),
			Self::V0Plus => Ok(Box::new(RemoteV0Plus::from(interface))),
			Self::V1 => Ok(Box::new(RemoteV1::from(interface))),
			Self::V2 => Ok(Box::new(RemoteV2::from(interface))),
			Self::V3 => Ok(Box::new(RemoteV3::from(interface))),
			Self::V4 => Ok(Box::new(RemoteV4::try_from(interface)?)),
			_ => todo!(),
		}
	}
}

impl Display for ProtocolVersion
{
	fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
	{
		match self {
			Self::Unknown => write!(fmt, "<unknown>"),
			Self::V0 => write!(fmt, "v0"),
			Self::V0Plus => write!(fmt, "v0+"),
			Self::V1 => write!(fmt, "v1"),
			Self::V2 => write!(fmt, "v2"),
			Self::V3 => write!(fmt, "v3"),
			Self::V4 => write!(fmt, "v4"),
		}
	}
}

impl Display for TargetArchitecture
{
	fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
	{
		let mut architectures = Vec::with_capacity(4);
		if self.contains(Self::CortexM) {
			architectures.push("ARM Cortex-M");
		}
		if self.contains(Self::CortexAR) {
			architectures.push("ARM Cortex-A/R");
		}
		if self.contains(Self::RiscV32) {
			architectures.push("RISC-V 32-bit");
		}
		if self.contains(Self::RiscV64) {
			architectures.push("RISC-V 64-bit");
		}
		write!(fmt, "{}", architectures.join(", "))
	}
}

impl Display for TargetFamily
{
	fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
	{
		let mut families = Vec::with_capacity(64);
		if self.contains(Self::AT32) {
			families.push("AteryTek AT32");
		}
		if self.contains(Self::Apollo3) {
			families.push("Ambiq Apollo3");
		}
		if self.contains(Self::CH32) {
			families.push("WinChipHead CH32");
		}
		if self.contains(Self::CH579) {
			families.push("WinChipHead CH579");
		}
		if self.contains(Self::EFM) {
			families.push("Energy Micro EFM32/EFR32/EZR32");
		}
		if self.contains(Self::GD32) {
			families.push("GigaDevice GD32");
		}
		if self.contains(Self::HC32) {
			families.push("HDSC HC32");
		}
		if self.contains(Self::LPC) {
			families.push("NXP LPC");
		}
		if self.contains(Self::MM32) {
			families.push("MindMotion MM32");
		}
		if self.contains(Self::NRF) {
			families.push("Nordi Semi nRF");
		}
		if self.contains(Self::NXPKinetis) {
			families.push("NXP/Freescale Kinetis");
		}
		if self.contains(Self::NXPiMXRT) {
			families.push("NXP i.MXRT");
		}
		if self.contains(Self::Puya) {
			families.push("Puya PY32");
		}
		if self.contains(Self::RenesasRA) {
			families.push("Renesas RA");
		}
		if self.contains(Self::RenesasRZ) {
			families.push("Renesas RZ");
		}
		if self.contains(Self::RP) {
			families.push("RPi Foundation RP2040/RP2350");
		}
		if self.contains(Self::SAM) {
			families.push("Atmel/Microchip ATSAM");
		}
		if self.contains(Self::STM) {
			families.push("ST Micro STM32");
		}
		if self.contains(Self::TI) {
			families.push("TI MSP432 and LM3S/TM4C");
		}
		if self.contains(Self::Xilinx) {
			families.push("Xilinx Zynq");
		}
		write!(fmt, "{}", families.join(", "))
	}
}
