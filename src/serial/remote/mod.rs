// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use color_eyre::eyre::Result;

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

/// Types implementing this trait implement the common portion of the BMD remote protocol
/// (this includes things like comms initialisation, and clock frequency control)
pub trait BmdRemoteProtocol
{
	// Comms protocol initialisation functions
	fn swd_init(&self) -> Result<Box<dyn BmdSwdProtocol>>;
	fn jtag_init(&self) -> Result<Box<dyn BmdJtagProtocol>>;
	// Higher level protocol initialisation functions
	fn adiv5_init(&self) -> bool;
	fn adiv6_init(&self) -> bool;
	fn riscv_jtag_init(&self) -> bool;

	// Probe operation control functions
	fn add_jtag_dev(&self, dev_index: u32, jtag_dev: &JtagDev);
	fn get_comms_frequency(&self) -> u32;
	fn set_comms_frequency(&self, freq: u32) -> bool;
	fn target_clk_output_enable(&self, enable: bool);
}

/// Types implementing this trait provide raw SWD access to targets over the BMD remote protocol
pub trait BmdSwdProtocol
{
	fn seq_in(&self, clock_cycles: usize) -> u32;
	fn seq_in_parity(&self, clock_cycles: usize) -> Option<u32>;
	fn seq_out(&self, value: u32, clock_cycles: usize);
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

pub enum ProtocolVersion
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
