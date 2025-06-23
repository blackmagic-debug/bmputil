// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

/// A version-agnostic Debug Module Interface on a RISC-V device
#[allow(unused)]
pub struct RiscvDmi
{
	/// DMI designer code
	designer_code: u16,
	/// Versioon of the spec this DMI implements
	version: RiscvDebugVersion,

	/// The index of this DMI on the JTAG chain if JTAG
	dev_index: u8,
	/// The number of bus idle cycles this DMI needs to complete transactions
	idle_cycles: u8,
	/// The address width of the DMI bus this DMI connects us to
	address_width: u8,
	/// Whether a fault has occured on the bus, and which one
	fault: u8,
}

/// RISC-V Debug spec versions that we know about
pub enum RiscvDebugVersion
{
	Unknown,
	Unimplemented,
	V0_11,
	V0_13,
	V1_0,
}
