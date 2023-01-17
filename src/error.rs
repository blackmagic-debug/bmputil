// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2023 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
//! Module for error handling code.

use std::fmt::{Display, Formatter};
#[cfg(feature = "backtrace")]
use std::backtrace::{Backtrace, BacktraceStatus};
use std::error::Error as StdError;

use thiserror::Error;

use crate::S;

/// More convenient alias for `Box<dyn StdError + Send + Sync>`,
/// which shows up in a few signatures and structs.
type BoxedError = Box<dyn StdError + Send + Sync>;

/// Kinds of errors for [Error]. Use [ErrorKind::error] and [ErrorKind::error_from] to generate the
/// [Error] value for this ErrorKind.
#[derive(Debug)]
pub enum ErrorKind
{
    /// Failed to read firmware file.
    FirmwareFileIo(/** filename **/ Option<String>),

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
            FirmwareFileIo(Some(filename)) => write!(f, "failed to read firmware file {}", filename)?,
            TooManyDevices => write!(f, "current operation only supports one Black Magic Probe device but more than one device was found")?,
            DeviceNotFound => write!(f, "Black Magic Probe device not found (check connection?)")?,
            DeviceDisconnectDuringOperation => write!(f, "Black Magic Probe device found disconnected")?,
            DeviceReboot => write!(f, "Black Magic Probe device did not come back online (invalid firmware?)")?,
            DeviceSeemsInvalid(thing) => {
                write!(
                    f,
                    "Black Magic Probe device returned bad data ({}) during configuration.\
                    This generally shouldn't be possible. Maybe cable is bad, or OS is messing with things?",
                    thing,
                )?;
            },
            InvalidFirmware(None) => write!(f, "specified firmware does not seem valid")?,
            InvalidFirmware(Some(why)) => write!(f, "specified firmware does not seem valid: {}", why)?,
            External(source) => {
                use ErrorSource::*;
                match source {
                    StdIo(e) => {
                        write!(f, "unhandled std::io::Error: {}", e)?;
                    },
                    Libusb(e) => {
                        write!(f, "unhandled libusb error: {}", e)?;
                    },
                    DfuLibusb(e) => {
                        write!(f, "unhandled dfu_libusb error: {}", e)?;
                    },
                    Goblin(e) => {
                        write!(f, "unhandled ELF parsing error: {}", e)?;
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

    /// Stores the backtrace for this error.
    ///
    /// Backtraces are apparently pretty large. This struct was 136 bytes without the box, which was annoying clippy.
    #[cfg(feature = "backtrace")]
    pub backtrace: Box<Backtrace>,

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
            context: None,
            #[cfg(feature = "backtrace")]
            backtrace: Box::new(Backtrace::capture()),
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

    #[cfg(feature = "backtrace")]
    #[allow(dead_code)]
    fn backtrace(&self) -> Option<&Backtrace>
    {
        Some(&self.backtrace)
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

        #[cfg(feature = "backtrace")]
        {
            if self.backtrace.status() == BacktraceStatus::Captured {
                write!(f, "\nBacktrace:\n{}", self.backtrace)?;
            }
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

impl From<rusb::Error> for Error
{
    fn from(other: rusb::Error) -> Self
    {
        use ErrorKind::*;
        match other {
            rusb::Error::NoDevice => DeviceNotFound.error_from(other),
            other => External(ErrorSource::Libusb(other)).error()
        }
    }
}

impl From<dfu_libusb::Error> for Error
{
    fn from(other: dfu_libusb::Error) -> Self
    {
        use ErrorKind::*;
        use dfu_libusb::Error as Source;
        match other {
            dfu_libusb::Error::LibUsb(source) => {
                External(ErrorSource::Libusb(source)).error_from(other)
            },
            dfu_libusb::Error::MemoryLayout(source) => {
                DeviceSeemsInvalid(String::from("DFU interface memory layout string"))
                    .error_from(source)
            },
            dfu_libusb::Error::MissingLanguage => {
                DeviceSeemsInvalid(S!("no string descriptor languages"))
                    .error_from(other)
            },
            Source::InvalidAlt => {
                DeviceSeemsInvalid(S!("DFU interface (alt mode) not found"))
                    .error_from(other)
            },
            Source::InvalidAddress => {
                DeviceSeemsInvalid(S!("DFU interface memory layout string"))
                    .error_from(other)
            },
            Source::InvalidInterface => {
                DeviceSeemsInvalid(S!("DFU interface not found"))
                    .error_from(other)
            },
            Source::InvalidInterfaceString => {
                DeviceSeemsInvalid(S!("DFU interface memory layout string"))
                    .error_from(other)
            },
            Source::FunctionalDescriptor(source) => {
                DeviceSeemsInvalid(S!("DFU functional interface descriptor"))
                    .error_from(source)
            },
            anything_else => {
                External(ErrorSource::DfuLibusb(anything_else))
                    .error()
            },
        }
    }
}

impl From<goblin::error::Error> for Error
{
    fn from(other: goblin::error::Error) -> Self
    {
        use ErrorKind::*;

        InvalidFirmware(None)
            .error_from(External(ErrorSource::Goblin(other)).error())
    }
}


/// Sources of external error in this library.
#[derive(Debug, Error)]
pub enum ErrorSource
{
    #[error(transparent)]
    StdIo(#[from] std::io::Error),

    #[error(transparent)]
    Libusb(#[from] rusb::Error),

    #[error(transparent)]
    DfuLibusb(#[from] dfu_libusb::Error),

    #[error(transparent)]
    Goblin(#[from] goblin::error::Error),
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
