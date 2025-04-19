// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
// SPDX-FileContributor: Modified by Rachel Mant <git@dragonmux.network>
//! Module for error handling code.

use std::{fmt::{Display, Formatter}, path::PathBuf};
use std::error::Error as StdError;

use thiserror::Error;

use crate::S;

/// More convenient alias for `Box<dyn StdError + Send + Sync>`,
/// which shows up in a few signatures and structs.
type BoxedError = Box<dyn StdError + Send + Sync>;

/// A unique type to hold Nusb error state
#[derive(Debug, Error)]
#[error(transparent)]
pub struct NusbError(#[from] nusb::Error);

/// Kinds of errors for [Error]. Use [ErrorKind::error] and [ErrorKind::error_from] to generate the
/// [Error] value for this ErrorKind.
#[derive(Debug)]
pub enum ErrorKind
{
    /// Failed to read firmware file.
    FirmwareFileIo(/** filename **/ Option<PathBuf>),

    /// Specified firmware seems invalid.
    InvalidFirmware(/** why **/ Option<String>),

    /// Current operation only supports one Black Magic Probe but more tha none device was found.
    TooManyDevices,

    /// Black Magic Probe device not found.
    DeviceNotFound,

    /// Black Magic Probe found disconnected during an ongoing operation.
    DeviceDisconnectDuringOperation,

    /// Black Magic Probe device did not come back online (e.g. after switching to DFU mode
    /// or flashing firmware).
    DeviceReboot,

    /// Black Magic Probe device returned bad data during configuration.
    ///
    /// This generally shouldn't be possible, but could happen if the cable is bad, the OS is
    /// messing with things, or the firmware on the device is corrupted.
    DeviceSeemsInvalid(/** invalid thing **/ String),

    /// Black Magic Debug release metadata was invalid in some way
    ReleaseMetadataInvalid,

    /// Unhandled external error.
    External(ErrorSource),
}

impl ErrorKind
{
    /// Creates a new [Error] from this error kind.
    ///
    /// Enables convenient code like:
    /// ```
    /// return Err(ErrorKind::DeviceNotFound.error());
    /// ```
    #[inline(always)]
    pub fn error(self) -> Error
    {
        Error::new(self, None)
    }

    /// Creates a new [Error] from this error kind, with the passed error as the source.
    ///
    /// Enables convenient code like:
    /// ```
    /// # let operation = || std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    /// operation().map_err(|e| ErrorKind::DeviceNotFound.error_from(e))?;
    /// ```
    #[inline(always)]
    pub fn error_from<E: StdError + Send + Sync + 'static>(self, source: E) -> Error
    {
        Error::new(self, Some(Box::new(source)))
    }
}

/// Constructs an [Error] for this [ErrorKind].
impl From<ErrorKind> for Error
{
    /// Constructs an [Error] for this [ErrorKind].
    fn from(other: ErrorKind) -> Self
    {
        other.error()
    }
}

impl Display for ErrorKind
{
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result
    {
        use ErrorKind::*;
        match self {
            FirmwareFileIo(None) => write!(f, "failed to read firmware file")?,
            FirmwareFileIo(Some(filename)) => write!(f, "failed to read firmware file {}", filename.to_string_lossy())?,
            TooManyDevices => write!(f, "current operation only supports one Black Magic Probe device but more than one device was found")?,
            DeviceNotFound => write!(f, "Black Magic Probe device not found (check connection?)")?,
            DeviceDisconnectDuringOperation => write!(f, "Black Magic Probe device found disconnected")?,
            DeviceReboot => write!(f, "Black Magic Probe device did not come back online (invalid firmware?)")?,
            DeviceSeemsInvalid(thing) => {
                write!(
                    f,
                    "\nBlack Magic Probe device returned bad data ({}) during configuration.\n\
                    This generally shouldn't be possible. Maybe cable is bad, or OS is messing with things?",
                    thing,
                )?;
            },
            InvalidFirmware(None) => write!(f, "specified firmware does not seem valid")?,
            InvalidFirmware(Some(why)) => write!(f, "specified firmware does not seem valid: {}", why)?,
            ReleaseMetadataInvalid => write!(f, "Black Magic Debug release metadata was mallformed")?,
            External(source) => {
                use ErrorSource::*;
                match source {
                    StdIo(e) => {
                        write!(f, "unhandled std::io::Error: {}", e)?;
                    },
                    Nusb(e) => {
                        write!(f, "unhandled nusb error: {}", e.0)?;
                    },
                    NusbTransfer(e) => {
                        write!(f, "unhandled nusb transfer error: {}", e)?;
                    }
                    DfuNusb(e) => {
                        write!(f, "unhandled dfu_nusb error: {}", e)?;
                    },
                    DfuCore(e) => {
                        write!(f, "unhandled dfu_core error: {}", e)?;
                    },
                };
            },
        };

        Ok(())
    }
}

#[derive(Debug)]
/// Error type for Black Magic Probe operations. Easily constructed from [ErrorKind].
pub struct Error
{
    pub kind: ErrorKind,
    pub source: Option<BoxedError>,

    /// A string for additional context about what was being attempted when this error occurred.
    ///
    /// Example: "reading current firmware version".
    pub context: Option<String>,
}

impl Error
{
    #[inline(always)]
    pub fn new(kind: ErrorKind, source: Option<BoxedError>) -> Self
    {
        Self {
            kind,
            source,
            context: None
        }
    }

    #[allow(dead_code)]
    /// Add additional context about what was being attempted when this error occurred.
    ///
    /// Example: "reading current firmware version".
    pub fn with_ctx(mut self, ctx: &str) -> Self
    {
        self.context = Some(ctx.to_string());
        self
    }

    #[allow(dead_code)]
    /// Removes previously added context.
    pub fn without_ctx(mut self) -> Self
    {
        self.context = None;
        self
    }
}

impl Display for Error
{
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result
    {
        if let Some(ctx) = &self.context {
            write!(f, "(while {}): {}", ctx, self.kind)?;
        } else {
            write!(f, "{}", self.kind)?;
        }

        if let Some(source) = &self.source {
            writeln!(f, "\nCaused by: {}", source)?;
        }

        Ok(())
    }
}

impl StdError for Error
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)>
    {
        self.source.as_deref().map(|e| e as &dyn StdError)
    }
}

impl From<std::io::Error> for Error
{
    fn from(other: std::io::Error) -> Self
    {
        use ErrorKind::*;
        ReleaseMetadataInvalid.error_from(External(ErrorSource::StdIo(other)).error())
    }
}

impl From<NusbError> for Error
{
    fn from(other: NusbError) -> Self
    {
        use ErrorKind::*;
        External(ErrorSource::Nusb(other)).error()
    }
}

impl From<nusb::transfer::TransferError> for Error
{
    fn from(other: nusb::transfer::TransferError) -> Self
    {
        use ErrorKind::*;
        use nusb::transfer::TransferError;

        match other {
            TransferError::Disconnected => DeviceDisconnectDuringOperation.error_from(other),
            other => External(ErrorSource::NusbTransfer(other)).error(),
        }
    }
}

impl From<nusb::descriptors::ActiveConfigurationError> for Error
{
    fn from(other: nusb::descriptors::ActiveConfigurationError) -> Self
    {
        use ErrorKind::*;
        DeviceSeemsInvalid("could not read active configuration for device".into()).error_from(other)
    }
}

impl From<dfu_nusb::Error> for Error
{
    fn from(other: dfu_nusb::Error) -> Self
    {
        use ErrorKind::*;
        use dfu_nusb::Error as Source;
        match other {
            Source::Nusb(source) => {
                External(ErrorSource::Nusb(NusbError(source))).error()
            },
            Source::AltSettingNotFound => {
                DeviceSeemsInvalid(S!("DFU interface (alt mode) not found"))
                    .error_from(other)
            },
            Source::FunctionalDescriptorNotFound => {
                DeviceSeemsInvalid(S!("DFU functional descriptor not found"))
                    .error_from(other)
            },
            Source::FunctionalDescriptor(source) => {
                DeviceSeemsInvalid(S!("DFU functional interface descriptor"))
                    .error_from(source)
            },
            anything_else => {
                External(ErrorSource::DfuNusb(anything_else))
                    .error()
            },
        }
    }
}

impl From<dfu_core::Error> for Error
{
    fn from(other: dfu_core::Error) -> Self
    {
        use ErrorKind::*;
        use dfu_core::Error as Source;
        match other {
            Source::MemoryLayout(source) => {
                DeviceSeemsInvalid(String::from("DFU interface memory layout string"))
                    .error_from(source)
            },
            Source::InvalidAddress => {
                DeviceSeemsInvalid(S!("DFU interface memory layout string"))
                    .error_from(other)
            },
            Source::InvalidInterfaceString => {
                DeviceSeemsInvalid(S!("DFU interface memory layout string"))
                    .error_from(other)
            },
            anything_else => {
                External(ErrorSource::DfuCore(anything_else))
                    .error()
            },
        }
    }
}

/// Sources of external error in this library.
#[derive(Debug, Error)]
pub enum ErrorSource
{
    #[error(transparent)]
    StdIo(#[from] std::io::Error),

    #[error(transparent)]
    Nusb(#[from] NusbError),

    #[error(transparent)]
    NusbTransfer(#[from] nusb::transfer::TransferError),

    #[error(transparent)]
    DfuNusb(#[from] dfu_nusb::Error),

    #[error(transparent)]
    DfuCore(#[from] dfu_core::Error),
}

/// Extension trait to enable getting the error kind from a Result<T, Error> with one method.
pub trait ResErrorKind<T>
{
    type Kind;
    fn err_kind(&self) -> Result<&T, &Self::Kind>;
}

impl<T> ResErrorKind<T> for Result<T, Error>
{
    type Kind = ErrorKind;

    fn err_kind(&self) -> Result<&T, &Self::Kind>
    {
        self.as_ref().map_err(|e| &e.kind)
    }
}


#[macro_export]
macro_rules! log_and_return
{
    ($err:expr) => {
        let err = $err;
        log::error!("{}", err);
        return Err(err);
    }
}
