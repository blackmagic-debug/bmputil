// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

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
	fn swd_init(&self) -> bool;
	fn jtag_init(&self) -> bool;
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
