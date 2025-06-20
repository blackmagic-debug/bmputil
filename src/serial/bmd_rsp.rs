// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::fs::File;
use std::io::Write;
use std::path::Path;

use color_eyre::eyre::Result;

pub struct BmdRspInterface
{
	handle: File,
	protocol_version: u64,
}

const REMOTE_START: &[u8] = b"+#!GA#";
pub const REMOTE_MAX_MSG_SIZE: usize = 1024;

impl BmdRspInterface
{
	pub fn from_path(serial_port: &Path) -> Result<Self>
	{
		// Get the serial interface to the probe open
		let handle = File::options().read(true).write(true).open(serial_port)?;

		// Construct an interface object
		let mut result = Self {
			handle,
			// Provide a dummy protocol version for the moment
			protocol_version: u64::MAX,
		};

		// Call the OS-specific handle configuration function to ready
		// the interface handle for use with the remote serial protocol
		result.init_handle()?;

		// Start remote protocol communications with the probe
		result.buffer_write(REMOTE_START)?;
		// let buffer = result.buffer_read(REMOTE_MAX_MSG_SIZE);

		// Now the object is ready to go, return it to the caller
		Ok(result)
	}

	fn buffer_write(&mut self, message: &[u8]) -> Result<()>
	{
		Ok(self.handle.write_all(message)?)
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
		Ok(())
	}
}

#[cfg(target_os = "windows")]
impl BmdRspInterface {}
