//! Rust-y wrappers around some of the WinAPI functions we're using.

use std::ptr;
use std::mem;
use std::iter::{self, Iterator};
use std::ffi::{c_void, OsStr};
use std::io::{Error as IoError, Result as IoResult};
use std::os::windows::prelude::OsStrExt;

use log::error;

use winapi::shared::minwindef::{DWORD, FALSE};
use winapi::shared::ntdef::HANDLE;
use winapi::shared::winerror::ERROR_INSUFFICIENT_BUFFER;
use winapi::um::setupapi::{DIGCF_ALLCLASSES, SP_DEVINFO_DATA};
use winapi::um::setupapi::{SetupDiGetClassDevsW, SetupDiEnumDeviceInfo, SetupDiGetDeviceRegistryPropertyA};
use winapi::um::handleapi::INVALID_HANDLE_VALUE;


/// Rust wrapper for [SP_DEVINFO_DATA](https://learn.microsoft.com/en-us/windows/win32/api/setupapi/ns-setupapi-sp_devinfo_data).
pub struct DevInfoData<'d>
{
    infoset: &'d DevInfoSet,
    pub raw: SP_DEVINFO_DATA,
}

impl<'d> DevInfoData<'d>
{
    /// Moves the raw struct into this one to create the wrapper.
    pub fn from_raw(infoset: &'d DevInfoSet, raw: SP_DEVINFO_DATA) -> Self
    {
        Self {
            infoset,
            raw,
        }
    }

    /// Retrieves a specified property as a Vec<u8> buffer.
    pub fn prop(&mut self, prop: DWORD) -> IoResult<Vec<u8>>
    {
        let mut size: DWORD = 0;

        let success = unsafe {
            SetupDiGetDeviceRegistryPropertyA(
                self.infoset.handle, // DeviceInfoSet.
                &mut self.raw, // DeviceInfoData.
                prop, // Property.
                ptr::null_mut(), // PropertyRegDataType. Allowed to be null.
                ptr::null_mut(), // PropertyBuffer. Allowed to be null if querying size.
                0, // PropertyBufferSize. Allowed to be 0 if querying size.
                &mut size, // RequiredSize. Used for querying size.
            )
        };

        if success == FALSE {

            // Windows gives ERROR_INSUFFICIENT_BUFFER even if all you're doing is querying the size.
            // So let's ignore that error for this call, but still consider other errors actual
            // errors.

            let e = IoError::last_os_error();
            if let Some(code) = e.raw_os_error() {
                if (code as DWORD) != ERROR_INSUFFICIENT_BUFFER {
                    return Err(e);
                }
            }
        }

        // Otherwise, `size` should be set to the buffer size we need for this property.
        // So allocate a buffer, and call the function again.

        let mut buffer = vec![0u8; size as usize];

        let success = unsafe {
            SetupDiGetDeviceRegistryPropertyA(
                self.infoset.handle, // DeviceInfoSet.
                &mut self.raw, // DeviceInfoData.
                prop, // Property.
                ptr::null_mut(), // PropertyRegDataType. Allowed to be null.
                buffer.as_mut_ptr(), // PropertyBuffer.
                size, // PropertyBufferSize.
                ptr::null_mut(), // RequiredSize. Only used for querying size.
            )
        };

        if success == FALSE {
            return Err(IoError::last_os_error());
        }

        Ok(buffer)
    }
}


/// Iterator of device info from a [DevInfoSet]. Created from [DevInfoSet::iter].
pub struct DevInfoIter<'d>
{
    infoset: &'d DevInfoSet,
    current_index: u32,
    err: Option<IoError>,
}

impl<'d> DevInfoIter<'d>
{
    /// Retrieves the most recent error from iteration, if any.
    #[allow(dead_code)]
    pub fn err(&self) -> Option<&IoError>
    {
        self.err.as_ref()
    }
}

/// If an error occurs during iteration, you can call [DevInfoIter::err] to get the error value.
impl<'d> Iterator for DevInfoIter<'d>
{
    type Item = DevInfoData<'d>;

    fn next(&mut self) -> Option<Self::Item>
    {
        let mut devinfo_data: SP_DEVINFO_DATA = unsafe { mem::zeroed() };
        devinfo_data.cbSize = mem::size_of::<SP_DEVINFO_DATA>() as u32;

        // Request the enumerated device info from Windows, based on our current index.
        let success = unsafe {
            SetupDiEnumDeviceInfo(self.infoset.handle, self.current_index, &mut devinfo_data)
        };

        // Increment the index *before* error handling, to avoid invalid state.
        self.current_index += 1;

        // If an error occurred fetching the devinfo data, set the struct's error and return None
        // for this iteration.
        if success == FALSE {
            let e = IoError::last_os_error();
            error!("Error occured on Windows devinfo enum iteration {}: {}", self.current_index, &e);
            self.err = Some(e);
            return None;
        }

        Some(DevInfoData::from_raw(&self.infoset, devinfo_data))
    }
}

/// Represents a Windows [Device Information Set](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/device-information-sets).
#[derive(Debug, Clone, PartialEq)]
pub struct DevInfoSet
{
    pub handle: HANDLE,
}

impl DevInfoSet
{
    pub fn from_enumerator(enumerator: &OsStr) -> IoResult<Self>
    {
        let enumerator: Vec<u16> = enumerator.encode_wide().chain(iter::once(0)).collect();

        let handle = unsafe {
            SetupDiGetClassDevsW(
                ptr::null_mut(), // ClassGuid
                enumerator.as_ptr(), // Enumerator.
                ptr::null_mut(), // hwndParent
                // Include all device classes.
                DIGCF_ALLCLASSES, // Flags.
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            return Err(IoError::last_os_error());
        }

        Ok(Self {
            handle,
        })
    }

    /// Iterates through all the devices in this info set, getting their
    /// [SP_DEVINFO_DATA](https://learn.microsoft.com/en-us/windows/win32/api/setupapi/ns-setupapi-sp_devinfo_data).
    pub fn iter(&self) -> DevInfoIter
    {
        DevInfoIter {
            infoset: self,
            current_index: 0,
            err: None,
        }
    }
}

pub fn get_prop_from_dev_info(infoset_handle: *mut c_void, devinfo: &mut SP_DEVINFO_DATA, prop: DWORD) -> IoResult<Vec<u8>>
{
    let mut size: DWORD = 0;

    let success = unsafe { SetupDiGetDeviceRegistryPropertyA(
        infoset_handle,
        devinfo,
        prop,
        ptr::null_mut(), // PropertyRegDataType.
        ptr::null_mut(), // PropertyBuffer.
        0, // PropertyBufferSize.
        &mut size, // RequiredSize.
    ) };

    if success == FALSE {

        // Ignore ERROR_INSUFFICIENT_BUFFER, since we're only trying to get the size right now.

        let e = IoError::last_os_error();
        if let Some(code) = e.raw_os_error() {
            if (code as DWORD) != ERROR_INSUFFICIENT_BUFFER {
                return Err(e);
            }
        }
    }

    let mut buffer = vec![0u8; size as usize];

    let success = unsafe { SetupDiGetDeviceRegistryPropertyA(
        infoset_handle,
        devinfo,
        prop,
        ptr::null_mut(),
        buffer.as_mut_ptr(),
        size,
        ptr::null_mut(),
    ) };

    if success == 0 {
        return Err(IoError::last_os_error());
    }

    Ok(buffer)
}
