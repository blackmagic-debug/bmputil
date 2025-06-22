// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use std::sync::Arc;

use crate::serial::remote::BmdAdiV5Protocol;

type TargetAddr32 = u32;
type TargetAddr64 = u64;

/// The ADIv5 debug port associated with a JTAG TAP or a SWD interface drop of an ARM debug based device
#[allow(unused)]
pub struct AdiV5DebugPort
{
	/// The index of the device on the JTAG chain or DP index on SWD
	dev_index: u8,
	/// Whether a fault has occured, and which one
	fault: u8,
	/// Bitfield of the DP's quirks such as if it's a minimal DP or has the duped AP bug
	quirks: u8,
	/// DP version
	version: u8,

	/// DPv2+ specific target selection value
	targetsel: u32,

	/// DP designer (not impplementer!)
	designer_code: u16,
	/// DP partno
	partno: u16,

	/// TARGETID designer, present on DPv2+
	target_designer_code: u16,
	/// TARGETID partno, present on DPv2+
	target_partno: u16,

	/// DPv3+ bus address width
	address_width: u8,

	/// The remote protocol implementation to talk to the DP against
	remote: Arc<dyn BmdAdiV5Protocol>,
}

/// An ADIv5 access port associated with an ADIv5 debug port on a device
#[allow(unused)]
pub struct AdiV5AccessPort
{
	/// The debug port this AP is asociated with
	dp: Arc<AdiV5DebugPort>,
	/// The AP's index on the DP
	index: u8,
	/// Flags associated with this AP such as whether the AP has system memory attached,
	/// or is 64-bit instead of (the default of) 32-bit
	flags: u8,

	/// The value read out from the ID register for this AP
	idr: u32,
	/// The base address of the ROM tables associated with this AP
	base: TargetAddr64,
	/// The Control and Status Word value associated with accessing this AP
	csw: u32,
	/// A copy of any attached Cortex-M core's DEMCR value when we first see the core
	cortexm_demcr: u32,

	/// AP designer code
	designer_code: u16,
	/// AP partno
	partno: u16,
}
