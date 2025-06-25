// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::fs::File;
use std::io::{Read, Write};
use std::mem::MaybeUninit;
use std::path::Path;
use std::sync::{Arc, Mutex};

use color_eyre::eyre::{Result, eyre};
use log::{debug, trace};

use crate::serial::remote::*;

pub struct BmdRspInterface
{
	handle: File,
	protocol_version: ProtocolVersion,

	read_buffer: [u8; REMOTE_MAX_MSG_SIZE],
	read_buffer_fullness: usize,
	read_buffer_offset: usize,
}

const REMOTE_START: &str = "+#!GA#";
const REMOTE_HL_CHECK: &str = "!HC#";

impl BmdRspInterface
{
	pub fn from_path(serial_port: &Path) -> Result<Self>
	{
		// Get the serial interface to the probe open
		debug!("Opening probe interface at {:?}", serial_port);
		let handle = File::options().read(true).write(true).open(serial_port)?;

		// Construct an interface object
		let mut result = Self {
			handle,
			// We start out by not knowing what version of protocol the probe talks
			protocol_version: ProtocolVersion::Unknown,

			// Initialise an empty read buffer to use for more efficiently reading
			// probe responses, being mindful that there's no good way to find out
			// how much data is waiting for us from the probe, so it's this or use
			// a read call a byte, which is extremely expensive!
			read_buffer: [0; REMOTE_MAX_MSG_SIZE],
			read_buffer_fullness: 0,
			read_buffer_offset: 0,
		};

		// Call the OS-specific handle configuration function to ready
		// the interface handle for use with the remote serial protocol
		result.init_handle()?;

		// Start remote protocol communications with the probe
		result.buffer_write(REMOTE_START)?;
		let buffer = result.buffer_read()?;
		// Check if that failed for any reason
		if buffer.is_empty() || buffer.as_bytes()[0] != REMOTE_RESP_OK {
			let message = if buffer.len() > 1 {
				&buffer[1..]
			} else {
				"unknown"
			};
			return Err(eyre!("Remote protocol startup failed, error {}", message));
		}
		// It did not, grand - we now have the firmware version string, so log it
		debug!("Remote is {}", &buffer[1..]);

		// Next, ask the probe for its protocol version number.
		// For historical reasons this is part of the "high level" protocol set, but is
		// actually a general request.
		result.buffer_write(REMOTE_HL_CHECK)?;
		let buffer = result.buffer_read()?;
		// Check for communication failures
		if buffer.is_empty() {
			return Err(eyre!("Probe failed to respond at all to protocol version request"));
		} else if buffer.as_bytes()[0] != REMOTE_RESP_OK && buffer.as_bytes()[0] != REMOTE_RESP_NOTSUP {
			// If the probe responded with anything other than OK or not supported, we're done
			return Err(eyre!("Probe responded improperly to protocol version request with {}", buffer));
		}
		// If the request failed by way of a not implemented response, we're on a v0 protocol probe
		if buffer.as_bytes()[0] == REMOTE_RESP_NOTSUP {
			result.protocol_version = ProtocolVersion::V0;
		} else {
			// Parse out the version number from the response
			let version = decode_response(&buffer[1..], 8);
			// Then decode/translate that to a protocol version enum value
			result.protocol_version = match version {
				// Protocol version number 0 coresponds to an enchanced v0 probe protocol ("v0+")
				0 => ProtocolVersion::V0Plus,
				1 => ProtocolVersion::V1,
				2 => ProtocolVersion::V2,
				3 => ProtocolVersion::V3,
				4 => ProtocolVersion::V4,
				_ => return Err(eyre!("Unknown remote protocol version {}", version)),
			};
		}
		trace!("Probe talks BMD RSP {}", result.protocol_version);

		// Now the object is ready to go, return it to the caller
		Ok(result)
	}

	/// Extract the remote protocol object to use to talk with this probe
	pub fn remote(self) -> Result<Box<dyn BmdRemoteProtocol>>
	{
		let interface = Arc::new(Mutex::new(self));
		let protocol = interface
			.lock()
			.map_err(|_| eyre!("Failed to aquire lock on interface to access remote protocol"))?
			.protocol_version;
		protocol.protocol_impl(interface.clone())
	}

	pub(crate) fn buffer_write(&mut self, message: &str) -> Result<()>
	{
		debug!("BMD RSP write: {}", message);
		Ok(self.handle.write_all(message.as_bytes())?)
	}

	pub(crate) fn buffer_read(&mut self) -> Result<String>
	{
		// First drain the buffer till we see a start-of-response byte
		let mut response = 0;
		while response != REMOTE_RESP {
			if self.read_buffer_offset == self.read_buffer_fullness {
				self.read_more_data()?;
			}
			response = self.read_buffer[self.read_buffer_offset];
			self.read_buffer_offset += 1;
		}

		// Now collect the response
		let mut buffer = [0u8; REMOTE_MAX_MSG_SIZE];
		let mut offset = 0;
		while offset < buffer.len() {
			// Check if we need more data or should use what's in the buffer already
			if self.read_buffer_offset == self.read_buffer_fullness {
				self.read_more_data()?;
			}
			// Look for an end of packet marker
			let mut response_length = 0;
			while self.read_buffer_offset + response_length < self.read_buffer_fullness &&
				offset + response_length < buffer.len()
			{
				if self.read_buffer[self.read_buffer_offset + response_length] == REMOTE_EOM {
					response_length += 1;
					break;
				}
				response_length += 1;
			}
			// We now either have a REMOTE_EOM or need all the data from the buffer
			let read_buffer_offset = self.read_buffer_offset;
			buffer[offset..offset + response_length]
				.copy_from_slice(&self.read_buffer[read_buffer_offset..read_buffer_offset + response_length]);
			self.read_buffer_offset += response_length;
			offset += response_length - 1;
			// If this was because of REMOTE_EOM, return
			if buffer[offset] == REMOTE_EOM {
				buffer[offset] = 0;
				let result = unsafe { String::from_utf8_unchecked(buffer[..offset].to_vec()) };
				debug!("BMD RSP read: {}", result);
				return Ok(result);
			}
			offset += 1;
		}
		// If we fell out here, we got what we could so return that..
		let result = unsafe { String::from_utf8_unchecked(buffer.to_vec()) };
		debug!("BMD RSP read: {}", result);
		Ok(result)
	}
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
impl BmdRspInterface
{
	fn init_handle(&self) -> Result<()>
	{
		use std::os::fd::AsRawFd;

		#[cfg(any(target_os = "linux", target_os = "android"))]
		use termios::os::linux::CRTSCTS;
		#[cfg(target_os = "macos")]
		use termios::os::macos::CRTSCTS;
		use termios::*;

		// Extract the current termios config for the handle
		let fd = self.handle.as_raw_fd();
		let mut attrs = Termios::from_fd(fd)?;

		// Reconfigure the attributes for 8-bit characters, no CTS/RTS hardware control flow,
		// w/ no model control signalling
		attrs.c_cflag &= !(CSIZE | CSTOPB);
		attrs.c_cflag |= CS8 | CLOCAL | CREAD | CRTSCTS;
		// Disable break character handling and turn off XON/XOFF based control flow
		attrs.c_iflag &= !(IGNBRK | IXON | IXOFF | IXANY);
		// Disable all signaling, echo, remapping and delays
		attrs.c_lflag = 0;
		attrs.c_oflag = 0;
		// Make reads not block, and set 0.5s for read timeout
		attrs.c_cc[VMIN] = 0;
		attrs.c_cc[VTIME] = 5;

		// Reconfigure the handle with the new termios config
		tcsetattr(fd, TCSANOW, &attrs)?;

		// Let the caller know that we successfully got done
		trace!("Configured comms handle to probe remote serial interface");
		Ok(())
	}

	fn read_more_data(&mut self) -> Result<()>
	{
		use std::os::fd::AsRawFd;
		use std::ptr::null_mut;

		use color_eyre::eyre::eyre;
		use libc::{FD_SET, FD_SETSIZE, FD_ZERO, c_int, fd_set, select, timeval};

		// Set up a FD set that describes our handle's FD
		let mut select_set = MaybeUninit::<fd_set>::uninit();
		unsafe {
			FD_ZERO(select_set.as_mut_ptr());
			FD_SET(self.handle.as_raw_fd(), select_set.as_mut_ptr());
		}
		let mut select_set = unsafe { select_set.assume_init() };

		// Wait for more data from the probe for up to 2 seconds
		let mut timeout = timeval {
			tv_sec: 2,
			tv_usec: 0,
		};
		let result = unsafe { select(FD_SETSIZE as c_int, &mut select_set, null_mut(), null_mut(), &mut timeout) };

		if result < 0 {
			// If the select call failed, bail
			Err(eyre!("Failed on select"))
		} else if result == 0 {
			// If we timed out then bail differently
			Err(eyre!("Timeout while waiting for BMD remote protocol response"))
		} else {
			// Otherwise we now know there's data, so try to fill the read buffer
			let bytes_received = self.handle.read(&mut self.read_buffer)?;
			trace!("Read {} bytes from probe", bytes_received);
			// Now we have more data, so update the read buffer counters
			self.read_buffer_fullness = bytes_received;
			self.read_buffer_offset = 0;
			Ok(())
		}
	}
}

#[cfg(target_os = "windows")]
impl BmdRspInterface
{
	const DCB_CHECK_PARITY: u32 = 1 << 1;
	const DCB_DSR_SENSITIVE: u32 = 1 << 6;
	const DCB_DTR_CONTROL_ENABLE: u32 = 1 << 4;
	const DCB_DTR_CONTROL_MASK: u32 = 3 << 4;
	const DCB_RTS_CONTROL_DISABLE: u32 = 0 << 12;
	const DCB_RTS_CONTROL_MASK: u32 = 3 << 12;
	const DCB_USE_CTS: u32 = 1 << 2;
	const DCB_USE_DSR: u32 = 1 << 3;
	const DCB_USE_XOFF: u32 = 1 << 9;
	const DCB_USE_XON: u32 = 1 << 8;

	fn init_handle(&self) -> Result<()>
	{
		use std::os::windows::io::AsRawHandle;

		use windows::Win32::Devices::Communication::{
			COMMTIMEOUTS, DCB, GetCommState, NOPARITY, PURGE_RXCLEAR, PurgeComm, SetCommState, SetCommTimeouts,
		};
		use windows::Win32::Foundation::HANDLE;

		// Extract the current CommState for the handle
		let handle = HANDLE(self.handle.as_raw_handle());
		let mut serial_params = MaybeUninit::<DCB>::uninit();
		let mut serial_params = unsafe {
			GetCommState(handle, serial_params.as_mut_ptr())?;
			serial_params.assume_init()
		};

		// Reconfigure and adjust device state to disable hardware flow control and
		// get it into the right mode for communications to work properly
		serial_params.ByteSize = 8;
		serial_params.Parity = NOPARITY;
		// The windows-rs crate exposes the bitfield parameters to us as a nebulous thing..
		// we hold local definitions for each of the values so we can turn them on and off
		// appropriately here. See <https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-dcb>
		// for where these values come from. When reading this particular bitfield, assume LSb to MSb
		// as one traverses down the structure.
		serial_params._bitfield &= !(Self::DCB_CHECK_PARITY |
			Self::DCB_USE_CTS |
			Self::DCB_USE_DSR |
			Self::DCB_DTR_CONTROL_MASK |
			Self::DCB_DSR_SENSITIVE |
			Self::DCB_USE_XOFF |
			Self::DCB_USE_XON |
			Self::DCB_RTS_CONTROL_MASK);
		serial_params._bitfield |= Self::DCB_DTR_CONTROL_ENABLE | Self::DCB_RTS_CONTROL_DISABLE;

		// Reconfigure the handle with the new communications state
		unsafe { SetCommState(handle, &serial_params)? };

		let timeouts = COMMTIMEOUTS {
			// Turn off read timeouts so that ReadFile() underlying File's read calls instantly returns
			// even if there's o data waiting (we implement our own mechanism below for that case as we
			// only want to wait if we get no data)
			ReadIntervalTimeout: u32::MAX,
			ReadTotalTimeoutMultiplier: 0,
			ReadTotalTimeoutConstant: 0,
			// Configure an exactly 100ms write timeout - we want this triggering to be fatal as something
			// has gone very wrong if we ever hit this.
			WriteTotalTimeoutMultiplier: 0,
			WriteTotalTimeoutConstant: 100,
		};
		unsafe {
			SetCommTimeouts(handle, &timeouts)?;

			// Having adjusted the line state, discard anything sat in the receive buffer
			PurgeComm(handle, PURGE_RXCLEAR)?;
		}

		// Let the caller know that we successfully got done
		trace!("Configured comms handle to probe remote serial interface");
		Ok(())
	}

	fn read_more_data(&mut self) -> Result<()>
	{
		use std::os::windows::io::AsRawHandle;

		use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
		use windows::Win32::System::Threading::WaitForSingleObject;
		use windows_result::Error;

		// Try to wait for up to 100ms for data to become available
		let handle = HANDLE(self.handle.as_raw_handle());
		if unsafe { WaitForSingleObject(handle, 100) } != WAIT_OBJECT_0 {
			return Err(eyre!("Timeout while waiting for BMD RSP response: {}", Error::from_win32()));
		}

		// Now we know there's data, so try to fill the read buffer
		let bytes_received = self.handle.read(&mut self.read_buffer)?;
		trace!("Read {} bytes from probe", bytes_received);
		// Now we have more data, so update the read buffer counters
		self.read_buffer_fullness = bytes_received;
		self.read_buffer_offset = 0;
		Ok(())
	}
}
