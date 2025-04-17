// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2023 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>

use std::fmt::{self, Display};
use std::path::PathBuf;

use nusb::DeviceInfo;
use thiserror::Error;

/// Simple newtype struct for some clarity in function arguments and whatnot.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Vid(pub u16);

/// Simple newtype struct for some clarity in function arguments and whatnot.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pid(pub u16);

/// Simple newtype struct for some clarity in function arguments and whatnot.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InterfaceClass(pub u8);
impl InterfaceClass
{
    /// bInterfaceClass field in DFU-class interface descriptors.
    ///
    /// \[[USB DFU Device Class Spec § 4.2.1, Table 4.1](https://usb.org/sites/default/files/DFU_1.1.pdf#page=12)
    /// and [§ 4.2.3, Table 4.4](https://usb.org/sites/default/files/DFU_1.1.pdf#page=15)\]
    pub const APPLICATION_SPECIFIC: Self = Self(0xFE);
}

/// Simple newtype struct for some clarity in function arguments and whatnot.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InterfaceSubClass(pub u8);
impl InterfaceSubClass
{
    /// bInterfaceSubClass field in DFU-class interface descriptors.
    ///
    /// \[[USB DFU Device Class Spec § 4.2.1, Table 4.1](https://usb.org/sites/default/files/DFU_1.1.pdf#page=12)
    /// and [§ 4.2.3, Table 4.4](https://usb.org/sites/default/files/DFU_1.1.pdf#page=15)\]
    pub const DFU: Self = Self(0x01);
}


/// Simple newtype struct for some clarity in function arguments and whatnot.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InterfaceProtocol(pub u8);
impl InterfaceProtocol
{
    /// bInterfaceProtocol field in DFU-class interface descriptors while in runtime mode.
    ///
    /// \[[USB DFU Device Class Spec § 4.2.1, Table 4.1](https://usb.org/sites/default/files/DFU_1.1.pdf#page=12)
    /// and [§ 4.2.3, Table 4.4](https://usb.org/sites/default/files/DFU_1.1.pdf#page=15)\]
    #[allow(dead_code)] // XXX
    pub const DFU_RUNTIME_MODE: Self = Self(0x01);

    /// bInterfaceProtocol field in DFU-class interface descriptors while in DFU mode.
    ///
    /// \[[USB DFU Device Class Spec § 4.2.1, Table 4.1](https://usb.org/sites/default/files/DFU_1.1.pdf#page=12)
    /// and [§ 4.2.3, Table 4.4](https://usb.org/sites/default/files/DFU_1.1.pdf#page=15)\]
    #[allow(dead_code)] // XXX
    pub const DFU_DFU_MODE: Self = Self(0x02);
}


/// Enum of request numbers for DFU class requests.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
pub enum DfuRequest
{
    Detach = 0,
    Dnload = 1,
    Upload = 2,
    GetStatus = 3,
    ClrStatus = 4,
    GetState = 5,
    Abort = 6,
}


/// Enum representing the two "modes" a DFU-class device can be in.
///
/// Runtime mode is the normal operation mode, in which a device does the things it's made for and
/// exposes all the necessary descriptors to do so.
/// DFU mode is limited operating mode used for firmware upgrade purposes *only*. Devices switch
/// into this mode at the host's request.
/// \[[USB DFU Device Class Spec § 4.1](https://usb.org/sites/default/files/DFU_1.1.pdf#page=11)
/// and [§ 4.2](https://usb.org/sites/default/files/DFU_1.1.pdf#page=14)\].
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum DfuOperatingMode
{
    Runtime,
    FirmwareUpgrade,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericDescriptorRef<'a>
{
    pub raw: &'a [u8],
}

impl<'a> GenericDescriptorRef<'a>
{
    /// Returns the first descriptor found.
    ///
    /// Panics if `bytes.len()` < `bytes[0]`.
    pub fn single_from_bytes(bytes: &'a [u8]) -> Self
    {
        let length = bytes[0] as usize;

        Self {
            raw: &bytes[0..length],
        }
    }

    /// Panics if any descriptors have an invalid size.
    pub fn multiple_from_bytes(bytes: &'a [u8]) -> Vec<Self>
    {
        let mut v: Vec<Self> = Vec::new();

        let mut current_bytes = &bytes[0..];

        loop {
            let descriptor = Self::single_from_bytes(current_bytes);
            let parsed_count = descriptor.length_usize();
            let remaining = current_bytes.len() - parsed_count;
            v.push(descriptor);
            if remaining == 0 {
                break;
            } else if remaining > 2 {
                current_bytes = &current_bytes[parsed_count..];
            } else {
                panic!("Descriptor seems to have an invalid size of {}!", remaining);
            }
        }

        v
    }

    #[allow(dead_code)] // XXX
    pub fn length(&self) -> u8
    {
        self.raw[0]
    }

    pub fn length_usize(&self) -> usize
    {
        self.raw[0] as usize
    }

    pub fn descriptor_type(&self) -> u8
    {
        self.raw[1]
    }
}


#[derive(Debug, Clone, PartialEq, Eq, Hash, Error)]
pub enum DescriptorConvertError
{
    #[error(
        "bLength field ({provided_length}) in provided data does not match the correct value\
        ({correct_length}) for this descriptor type"
    )]
    LengthFieldMismatch
    {
        provided_length: u8,
        correct_length: u8,
    },

    #[error(
        "bDescriptorType field ({provided_type}) in provided data does not match the correct\
        value ({correct_type}) for this descriptor type"
    )]
    DescriptorTypeMismatch
    {
        provided_type: u8,
        correct_type: u8,
    },
}


/// Structure of the DFU-class functional descriptor.
///
/// Unfortunately, as this structure contains `u16`s at uneven offsets, making this struct
/// `repr(packed)` would allow you to easily create unaligned references, and thus this
/// struct does not match the memory layout of the data sent over the USB bus. Sadface indeed.
#[allow(non_snake_case)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct DfuFunctionalDescriptor
{
    pub bLength: u8, // Should be 0x09.
    pub bDescriptorType: u8, // Should be 0x21.
    pub bmAttributes: u8,
    pub wDetachTimeOut: u16,
    pub wTransferSize: u16,
    pub bcdDFUVersion: u16,
}

impl DfuFunctionalDescriptor
{
    pub const LENGTH: u8 = 0x09;
    pub const TYPE: u8 = 0x21;

    /// Constructs a [DfuFunctionalDescriptor] from a byte slice, via per-field copy.
    pub fn copy_from_bytes(bytes: &[u8; 0x09]) -> Result<Self, DescriptorConvertError>
    {
        if bytes[0] != Self::LENGTH {
            return Err(DescriptorConvertError::LengthFieldMismatch {
                provided_length: bytes[0],
                correct_length: Self::LENGTH,
            });
        }

        if bytes[1] != Self::TYPE {
            return Err(DescriptorConvertError::DescriptorTypeMismatch {
                provided_type: bytes[0],
                correct_type: Self::TYPE,
            });
        }

        Ok(Self {
            bLength: bytes[0],
            bDescriptorType: bytes[1],
            bmAttributes: bytes[2],
            wDetachTimeOut: u16::from_le_bytes(bytes[3..=4].try_into().unwrap()),
            wTransferSize: u16::from_le_bytes(bytes[5..=6].try_into().unwrap()),
            bcdDFUVersion: u16::from_le_bytes(bytes[7..=8].try_into().unwrap()),
        })
    }
}

// Abstraction of an arbitrary nusb device's location on the host system
#[derive(Debug, Eq, Clone)]
pub struct PortId
{
    bus_number: u8,
    #[cfg(any(target_os = "linux", target_os = "android"))]
    path: PathBuf,
}

impl PortId
{
    pub fn new(device: &DeviceInfo) -> Self
    {
        Self {
            bus_number: device.bus_number(),
            #[cfg(any(target_os = "linux", target_os = "android"))]
            path: device.sysfs_path().to_path_buf(),
        }
    }
}

impl PartialEq for PortId
{
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn eq(&self, other: &Self) -> bool
    {
        return self.bus_number == other.bus_number &&
            self.path == other.path
    }
}

impl Display for PortId
{
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
    {
        let port = self.path.file_name()
            .map_or_else(
                || Ok("Invalid PortId (bad path)".into()),
                |name| name.to_os_string().into_string()
            );

        match port {
            Ok(port) => write!(f, "{}", port),
            Err(_) => Err(fmt::Error)
        }
    }
}
