// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2023 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
//! This module handles Windows-specific code, mostly the installation of drivers for Black Magic Probe USB interfaces.
//!
//! "Installation" is a somewhat overloaded term, in Windows. Behind the scenes this module, using
//! [libwdi](https://github.com/pbatard/libwdi) (with [wdi-rs](https://github.com/Qyriad/wdi-rs) as the Rust interface),
//! adds an [INF file](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/overview-of-inf-files)
//! that binds [WinUSB.sys](https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/winusb-installation) to
//! the Windows device nodes for the app-mode Black Magic Probe VID/PID DFU interface (as interfaces get their own
//! devnodes in Windows) and the DFU-mode VID/PID (which has no interfaces). The INF file, as far as I can tell, gets
//! placed to `C:\Windows\INF`, with a name like `oem10.inf`. The Windows Registry gets a value that corresponds to
//! this at `HKLM:\SYSTEM\DriverDatabase\DeviceIds\USB\{hwid}`, with `hwid` being `VID_1D50&PID_6018&MI_04` for the
//! the normal mode BMP device (meaning "USB VID 0x1d50, USB PID 0x6018, interface index 4"), and `VID_1D50&6017` for
//! the DFU mode BMP device (meaning "VID 0x1d50, PID 0x6017, no interface"). That key gets a value of the same name as
//! the INF file, so something like `oem10.inf`. Note that this works *even if the BMP has never been plugged in*, but
//! will *not* show up in Device Manager until you *do* plug it in, even if you enable "Show hidden devices".
//! We also use this registry key to detect if the driver has already been installed. In the future, we should probably
//! add some command that attempts to repair a possibly broken driver installation by checking the INF file the Registry
//! key refers to.
//!
//! [ensure_access] is the main driving function for this module. It checks the above registry key to determine if the
//! drivers have already been installed, and orchestrates the installation itself. libwdi is where most of the magic
//! happens though — it handles generating the INF file and calling the relevant Windows
//! [SetupAPI](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/setupapi) functions to actually move
//! the INF to the right directory and create the right Registry keys.

use std::ffi::c_void;
use std::ptr;
use std::mem;
use std::env;
use std::iter;
use std::thread;
use std::str::FromStr;
use std::time::Duration;
use std::io::{Error as IoError, Result as IoResult};
use std::ffi::{OsStr, OsString, CString};
use std::os::windows::ffi::OsStrExt;

use libc::{intptr_t, c_int, c_uint, c_long, c_char, FILE};
use log::{trace, debug, info, warn, error};
use bstr::ByteSlice;
use lazy_static::lazy_static;
use winreg::enums::*;
use winreg::RegKey;

use winapi::um::wincon::{FreeConsole, AttachConsole};
#[allow(unused_imports)]
use winapi::um::winbase::{STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE};
use winapi::um::consoleapi::AllocConsole;
use deelevate::{Token, PrivilegeLevel};

/// From fnctl.h
/// ```c
/// #define _O_TEXT        0x4000  // file mode is text (translated)
/// ```
const _O_TEXT: c_int = 0x4000;

#[allow(dead_code)]
const STDIN_FILENO: c_int = 0;
#[allow(dead_code)]
const STDOUT_FILENO: c_int = 1;
#[allow(dead_code)]
const STDERR_FILENO: c_int = 2;


extern "C"
{
    /// https://docs.microsoft.com/en-us/cpp/c-runtime-library/reference/open-osfhandle?view=msvc-170
    pub fn _open_osfhandle(osfhandle: intptr_t, flags: c_int) -> c_int;

    /// https://docs.microsoft.com/en-us/cpp/c-runtime-library/reference/fdopen-wfdopen?view=msvc-170
    pub fn _fdopen(fd: c_int, mode: *const c_char) -> *mut FILE;

    /// https://docs.microsoft.com/en-us/cpp/c-runtime-library/reference/dup-dup2?view=msvc-170
    pub fn _dup2(fd1: c_int, fd2: c_int) -> c_int;

    /// An internal CRT function that Windows uses to define stdout, stderr, and stdin.
    /// ```c
    /// _ACRTIMP_ALT FILE* __cdecl __acrt_iob_func(unsigned _Ix);
    /// ```
    pub fn __acrt_iob_func(_Ix: c_uint) -> *mut FILE;
}

#[allow(dead_code)]
pub fn stdinf() -> *mut FILE
{
    unsafe { __acrt_iob_func(0) }
}

pub fn stdoutf() -> *mut FILE
{
    unsafe { __acrt_iob_func(1) }
}

pub fn stderrf() -> *mut FILE
{
    unsafe { __acrt_iob_func(2) }
}


/// Macro for calling winapi functions that return a BOOL to indicate success.
/// Transforms the return value into a Rust-y Result.
///
/// In this case, a return value of 0 is considered an error.
#[allow(unused_macros)]
macro_rules! winapi_bool
{
    ($e:expr) => {
        match $e {
            0 => Err::<(), _>(IoError::last_os_error()),
            _ => Ok(()),
        }
    }
}


/// Macro for calling winapi functions that return a HANDLE.
/// Transforms the return value into a Rust-y Result.
///
/// In this case, a return value of INVALID_HANDLE_VALUE is considered an error.
#[allow(unused_macros)]
macro_rules! winapi_handle
{
    ($e:expr) => {
        match $e {
            winapi::um::handleapi::INVALID_HANDLE_VALUE => Err::<winapi::um::winnt::HANDLE, _>(IoError::last_os_error()),
            handle => Ok(handle),
        }
    }
}

/// Macro for calling winapi functions that return `-1` to indicate failure.
/// Transforms the return value into a Rust-y Result.
///
/// In this case, a return value of -1 is considered an error.
#[allow(unused_macros)]
macro_rules! winapi_neg
{
    ($e:expr) => {
        match $e {
            -1 => Err(IoError::last_os_error()),
            other => Ok(other),
        }
    }
}


/// Internal struct for FILE* on Windows. See [restore_cstdio]'s implementation for details.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct UcrtStdioStreamData
{
    _ptr: *mut FILE,
    _base: *mut i8,
    _cnt: c_int,
    _flags: c_long,
    /// Note: this is what is returned by _fileno()
    _file: c_long,
    _charbuf: c_int,
    _bufsiz: c_int,
    _tmpfname: *mut i8,
    _lock: *mut c_void,
}



/// When our admin process is created, it does not inherit stdin, stdout, and stderr from the parent process.
/// AttachConsole(parent_pid) easily connects the admin process to the parent console, but, surprisingly
/// enough, that only restores stdio for *Rust*, and not C. How could this possibly be the case?
/// Well Rust's e.g. println!() eventually calls WinAPI's `WriteConsoleW()` using the console handle.
/// C's printf() on the other hand goes through whatever file descriptor the `stdout` global is set to,
/// and that is *not* updated when you call AttachConsole(). So, we need to resynchronize the
/// Microsoft C Runtime's stdio global state with the Windows console subsystem state.
pub fn restore_cstdio(parent_pid: u32) -> Result<(), IoError>
{

    // First, free whatever console Windows gave us by default, and attach to the parent's console.
    unsafe {
        FreeConsole();
        // Why not ATTACH_PARENT_PROCESS? Well it worked on my machine, but didn't work in a VM.
        // This seems to work better, for some reason.
        if let Err(_e) = winapi_bool!(AttachConsole(parent_pid)) {
            // If we can't attach the previous console, then allocate a new console instead.
            // This will pop up a new window for the user, but that's better than no output
            // at all.
            AllocConsole();
        }
    }

    // Resync for each of stdin, stdout, and stderr.

    let res = unsafe { libc::freopen(b"CONIN$\0".as_ptr() as *const i8, b"w\0".as_ptr() as *const i8, stdinf()) };
    if res.is_null() {
        Err::<(), _>(IoError::last_os_error())
            .expect("Failed to resynchronize stdin");
    }

    let res = unsafe { libc::freopen(b"CONOUT$\0".as_ptr() as *const i8, b"wt\0".as_ptr() as *const i8, stdoutf()) };
    if res.is_null() {
        Err::<(), _>(IoError::last_os_error())
            .expect("Failed to resynchronize stdout");
    }

    // HACK: on some¹ systems, using the same technique for stderr seems to break both stderr and stdout, for some
    // reason. So instead we'll copy the internal FILE* structure used for stdout to the stderr global.
    //
    // ¹It worked just fine on my personal dev machine and one other personal Windows machine, but didn't work in a
    // fresh VM, so who knows.
    let out = stdoutf() as *mut UcrtStdioStreamData;
    let err = stderrf() as *mut UcrtStdioStreamData;
    unsafe {
        *err = *out;
    }

    Ok(())
}


fn os_str_to_null_terminated_vec(s: &OsStr) -> Vec<u16>
{
    s.encode_wide().chain(iter::once(0)).collect()
}


/// Install drivers for each libwdi [wdi::DeviceInfo] in `devices`. Must be called from admin.
fn admin_install_drivers(devices: &mut [wdi::DeviceInfo])
{
    // TODO: cd into a tempdir so libwdi doesn't spill files into the user's cwd?

    // NOTE: the sleeps in this function are a mitigation for inconsistent errors I've had when
    // testing this. Windows doesn't always seem to like doing all of these operations in quick
    // succession, and sometimes, somehow, the process seems to lock itself(?) out of accessing the
    // intermediate files libwdi creates for this.

    for dev in devices.into_iter() {

        let hwid_str = dev.hardware_id
            .as_ref()
            .expect("BMP WDI DeviceInfo always have hardware_id set")
            .to_str_lossy()
            .to_string();

        println!("Installing for {}", &hwid_str);

        thread::sleep(Duration::from_secs(1));

        println!("Preparing driver for installation...");

        wdi::prepare_driver(dev, "usb_driver", "usb_device.inf", &mut Default::default())
            .unwrap();

        println!("Driver prepared.");
        println!("About to install driver. This may take multiple minutes and there will be NO PROGRESS REPORTING!");
        println!("Installing driver...");

        thread::sleep(Duration::from_secs(1));

        wdi::install_driver(dev, "usb_driver", "usb_device.inf", &mut Default::default())
            .unwrap();

        println!("Driver successfully installed for {}", &hwid_str);
    }
}


lazy_static! {
    pub static ref APP_MODE_WDI_INFO: wdi::DeviceInfo = wdi::DeviceInfo {
        vid: 0x1d50,
        pid: 0x6018,
        is_composite: true,
        mi: 4,
        //desc: String::from("Black Magic DFU (Interface 4)").into(),
        desc: CString::new("Black Magic DFU (Interface 4)").unwrap().into_bytes_with_nul().to_vec(),
        driver: None,
        device_id: None,
        hardware_id: Some(CString::new(r"USB\VID_1D50&PID_6018&REV_0100&MI_04").unwrap().to_bytes_with_nul().to_vec()),
        compatible_id: Some(CString::new(r"USB\Class_fe&SubClass_01&Prot_01").unwrap().to_bytes_with_nul().to_vec()),
        upper_filter: None,
        driver_version: 0,
    };

    pub static ref DFU_MODE_WDI_INFO: wdi::DeviceInfo = wdi::DeviceInfo {
        vid: 0x1d50,
        pid: 0x6017,
        is_composite: false,
        mi: 0,
        desc: CString::new("Black Magic Probe DFU").unwrap().to_bytes_with_nul().to_vec(),
        driver: None,
        device_id: None,
        hardware_id: Some(CString::new(r"USB\VID_1D50&PID_6017&REV_0100").unwrap().to_bytes_with_nul().to_vec()),
        compatible_id: Some(CString::new(r"USB\Cass_FE&SubClass_01&Prot_02").unwrap().to_bytes_with_nul().to_vec()),
        upper_filter: None,
        driver_version: 0,
    };
}


/// Checks what drivers a device with a given [enumerator] and [HardwareId] is bound to, if any, via the Windows registry.
/// Returns the INF names of any bound drivers.
///
/// `hardware_id` should *not* include the enumerator name. e.g. no leading `USB\`.
///
/// This function checks `HKLM:\SYSTEM\DriverDatabase\DeviceIds\{enumerator}\{hardware_id}`.
/// The Windows registry is case in-sensitive.
///
/// [enumerator]: (https://learn.microsoft.com/en-us/windows/win32/api/setupapi/nf-setupapi-setupdigetclassdevsw)
/// [HardwareId]: (https://learn.microsoft.com/en-us/windows-hardware/drivers/install/hardware-ids)
pub fn hwid_bound_to_driver(hardware_id: &str, enumerator: &str) -> IoResult<Vec<String>>
{
    debug!("Checking what drivers device {} under enumerator {} is bound to", hardware_id, enumerator);

    let mut driver_names: Vec<String> = Vec::new();

    let hwid_lower = hardware_id.to_lowercase();

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);


    // Open the driver database for the given enumerator.

    // The registry key name for a driver database for `enumerator` is
    // `HKLM:\SYSTEM\DriverDatabase\DeviceIds\{enumerator}`.
    let mut driver_db_subkey_name = String::from(r"SYSTEM\DriverDatabase\DeviceIds\");
    driver_db_subkey_name.push_str(enumerator);

    trace!(r"Opening HKLM:\{}", &driver_db_subkey_name);
    let driver_db_for_enum = hklm.open_subkey(&driver_db_subkey_name)
        .map_err(|e| {
            error!("Error opening USB driver database in registry: {}", &e);
            e
        })?;


    // Subkey of the driver DB are Windows HardwareId strings, without the leading enumerator name.

    for dev_key_name in driver_db_for_enum.enum_keys() {
        let dev_key_name: String = dev_key_name
            .map_err(|e| {
                warn!("Error enumerating registry subkeys of {:?}: {}", &driver_db_for_enum, &e);
                e
            })?;

        let dev_key_lower = dev_key_name.to_lowercase();

        // If this subkey has the same name as the hardware ID we're looking for...
        if dev_key_lower == hwid_lower {

            // Then check if it has any drivers listed, or if it's empty.
            // Usually, however, unbound devices will not have a subkey of their enumerator at all.

            trace!(r"Opening HKLM:\{}\{}", &driver_db_subkey_name, &dev_key_name);
            let dev_key = driver_db_for_enum.open_subkey(&dev_key_name)
                .map_err(|e| {
                    warn!("Error opening know-to-exist subkey {:?} when checking bound drivers: {}", &dev_key_name, &e);
                    e
                })?;

            for driver_regval in dev_key.enum_values() {

                let (driver_name, _driver_value) = driver_regval
                    .map_err(|e| {
                        warn!(
                            "Error enumerating values of key {:?} when checking bound drivers: {}",
                            &dev_key,
                            &e,
                        );
                        e
                    })?;

                // Values of `HKLM:\SYSTEM\DriverDatabase\DeviceIds\{enumerator}\{hwid}` are of type
                // REG_BINARY, but I have no idea what the format is. The name, however, is the name of
                // the INF file bound to the device, e.g. `oem15.inf`, and that's what we want.
                driver_names.push(driver_name);

            }

        }
    }

    Ok(driver_names)
}


/// This function ensures that all connected Black Magic Probe devices have the necessary drivers installed, via libwdi.
/// If `explicitly_requested` is true, then this will print if there is nothing to do.
/// If `force` is true, then this will install even if there is an existing driver.
// FIXME: This should return a Result, and should probably return what devices had drivers
pub fn ensure_access(parent_pid: Option<u32>, explicitly_requested: bool, force: bool)
{
    // Check if the WinUSB driver has been installed for BMP devices yet.

    debug!("Checking Windows registry driver database to determine if WinUSB is bound to BMP device nodes");

    let mut devices_needing_driver: Vec<wdi::DeviceInfo> = Vec::with_capacity(2);

    if force {
        info!("Force installing WinUSB driver for app mode and DFU mode BMP devices...");
        devices_needing_driver.push(APP_MODE_WDI_INFO.clone());
        devices_needing_driver.push(DFU_MODE_WDI_INFO.clone());
    } else {

        match hwid_bound_to_driver("VID_1D50&PID_6018&MI_04", "USB") {
            Ok(driver_names) if driver_names.len() == 0 => {
                devices_needing_driver.push(APP_MODE_WDI_INFO.clone());
                info!("Scheduling WinUSB driver installation for app mode BMP device...");
            }

            // If an error occurred checking, then install the driver just in case.
            Err(_e) => {
                devices_needing_driver.push(APP_MODE_WDI_INFO.clone());
                info!("Scheduling WinUSB driver installation for app mode BMP device...");
            }

            Ok(driver_names) => {
                trace!("App mode BMP bound to drivers: {:?}", driver_names);
            },
        }

        match hwid_bound_to_driver("VID_1D50&PID_6017", "USB") {
            Ok(driver_names) if driver_names.len() == 0 => {
                devices_needing_driver.push(DFU_MODE_WDI_INFO.clone());
                info!("Scheduling WinUSB driver installation for DFU mode BMP device...");
            }

            // If an error occurred checking, then install the driver just in case.
            Err(_e) => {
                devices_needing_driver.push(DFU_MODE_WDI_INFO.clone());
                info!("Scheduling WinUSB driver installation for DFU mode BMP device...");
            }

            Ok(driver_names) => {
                trace!("DFU mode BMP bound to drivers: {:?}", driver_names);
            },
        }

    }

    // If both drivers are installed already, there's nothing to do.
    if devices_needing_driver.len() == 0 {
        if explicitly_requested {
            println!("Drivers are already installed for BMP devices; nothing to do.");
        }
        return;
    }


    println!("The WinUSB driver needs to be installed for the Black Magic Probe device before continuing. Standby...");
    thread::sleep(Duration::from_secs(1));

    // If we're here, that means we're installing drivers.
    // So we need admin.
    let token = Token::with_current_process()
        .expect("Unable to determine the current process's privilege level");
    let level = token.privilege_level()
        .expect("Unable to determine the current process's privilege level");

    let need_to_elevate = match level {
        PrivilegeLevel::NotPrivileged | PrivilegeLevel::HighIntegrityAdmin => true,
        _ => {
            false
        },
    };

    if !need_to_elevate {

        if let Some(pid) = parent_pid {
            match restore_cstdio(pid) {
                Ok(_) => (),
                Err(_e) => {
                    // FIXME:
                    todo!("Create a log file!");
                }
            }
        }

        admin_install_drivers(&mut devices_needing_driver);
        println!("Successfully installed drivers for {} USB interfaces.", devices_needing_driver.len());

        // TODO: use the Windows SetupAPI to get the device instance ID of the BMP so we can restart it and re-enumerate it, if necessary.
        // https://docs.microsoft.com/en-us/windows/win32/api/setupapi/nf-setupapi-setupdigetdeviceinstanceida

        // Now that we're done, nothing more to do in the admin process.
        std::process::exit(0);
    }

    // If we need to elevate, then we have to re-execute this process.

    // FIXME: this elevated-execution code should be cleaned up.

    let mut args: Vec<OsString> = Vec::with_capacity(env::args_os().len() + 1);
    args.extend(env::args_os().map(|s| s.to_owned()));
    args.push(OsString::from_str("--windows-wdi-install-mode").unwrap());

    use winapi::um::winbase;
    use winapi::um::winuser;
    use winapi::um::shellapi;
    use winapi::um::shellapi::SHELLEXECUTEINFOW;
    use winapi::um::shellapi::ShellExecuteExW;
    use winapi::um::synchapi;

    let verb: Vec<u16> = OsStr::new("runas").encode_wide().chain(iter::once(0)).collect();

    let mut args: Vec<OsString> = env::args_os().map(|s| s.to_owned()).collect();
    // Remove argv[0], as we're going to replace it with the full path to the process.
    let _ = args.remove(0);
    args.push(OsStr::new(&format!("--windows-wdi-install-mode={}", std::process::id())).to_owned());
    let file = os_str_to_null_terminated_vec(env::current_exe().unwrap().as_os_str());
    let parameters: OsString = args
        .join(OsStr::new(" "));
    let parameters = os_str_to_null_terminated_vec(&parameters);

    let cwd = os_str_to_null_terminated_vec(
        env::current_dir()
            .expect("Unable to get current working directory")
            .as_os_str()
    );

    let mut info = SHELLEXECUTEINFOW {
        cbSize: mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: shellapi::SEE_MASK_NOCLOSEPROCESS,
        hwnd: ptr::null_mut(),
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: parameters.as_ptr(),
        lpDirectory: cwd.as_ptr(),
        nShow: winuser::SW_HIDE,
        hInstApp: ptr::null_mut(),
        lpIDList: ptr::null_mut(),
        lpClass: ptr::null_mut(),
        hkeyClass: ptr::null_mut(),
        hMonitor: ptr::null_mut(),
        dwHotKey: 0,
        hProcess: ptr::null_mut(),
    };

    let res = unsafe { ShellExecuteExW(&mut info) };
    if res == winapi::shared::minwindef::FALSE {
        Err::<(), _>(IoError::last_os_error())
            .expect("Error calling ShellExecuteExW()");
    }

    let hproc = info.hProcess;
    let ret = unsafe { synchapi::WaitForSingleObject(hproc, winbase::INFINITE) };
    if ret == winbase::WAIT_FAILED {
        Err::<(), _>(IoError::last_os_error())
            .expect("Error calling WaitForSingleObject()");
    }
    std::thread::sleep(std::time::Duration::from_secs(5));

    let mut exit_code = 0;
    if unsafe { winapi::um::processthreadsapi::GetExitCodeProcess(hproc, &mut exit_code) } != 0 {
        if exit_code != 0 {
            error!("Elevated process exited with {}; driver installation probably failed", exit_code);
            std::process::exit(exit_code as i32);
        } else {
            info!("Exiting parent process. Elevated process exited successfully.");
        }
    } else {
        Err::<(), _>(IoError::last_os_error())
            .expect("Error calling GetExitCodeProcess()");
    }

    println!(
        "Driver installation should be complete. \
        You may need to unplug the device and plug it back in before things work."
    );
}
