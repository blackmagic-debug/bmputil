use std::ffi::c_void;
use std::ptr;
use std::mem;
use std::env;
use std::iter;
use std::str::FromStr;
use std::io::Error as IoError;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStrExt;

use libc::{intptr_t, c_int, c_uint, c_long, c_char, FILE};
use log::{info, warn};

use winapi::um::wincon::{FreeConsole, AttachConsole};
#[allow(unused_imports)]
use winapi::um::winbase::{STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE};
use winapi::um::consoleapi::AllocConsole;
use deelevate::{Token, PrivilegeLevel};

use crate::usb::{Vid, Pid, DfuMatch};
use crate::bmp::BmpVidPid;


/// From fnctl.h
/// ```c
/// #define _O_TEXT        0x4000  // file mode is text (translated)
/// ```
const _O_TEXT: c_int = 0x4000;

#[allow(dead_code)]
const STDIN_FILENO: c_int = 0;
const STDOUT_FILENO: c_int = 1;
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


fn admin_install_drivers(devices: &mut [wdi::DeviceInfo])
{
    for dev in devices.into_iter() {

        std::thread::sleep(std::time::Duration::from_secs(1));

        println!("Preparing driver for installation...");

        wdi::prepare_driver(dev, "usb_driver", "usb_device.inf", &mut Default::default())
            .unwrap();

        println!("Driver prepared.");
        println!("About to install driver. This may take multiple minutes and there will be NO PROGRESS REPORTING!");
        println!("Installing driver...");

        wdi::install_driver(dev, "usb_driver", "usb_device.inf", &mut Default::default())
            .unwrap();
    }
}



/// This function ensures that all connected Black Magic Probe devices have the necessary drivers installed, via libwdi.
pub fn ensure_access(parent_pid: Option<u32>)
{
    // Find all driverless Black Magic Probe DFU "devices"
    // (interfaces are considered devices in Windows terminology).

    let devices: Result<_, _> = wdi::create_list(Default::default());

    // If no devices were found at all, return.
    if let Err(wdi::Error::NoDevice) = devices {
        return;
    }

    let mut devices: Vec<_> = devices
        .expect("Unable to get a list of connected devices")
        .into_iter()
        .filter(|d| {
            BmpVidPid::mode_from_vid_pid(Vid(d.vid), Pid(d.pid))
                .is_some()
                &&
                match d.compatible_id.as_ref() {
                    Some(compatible_id) => {
                        // Windows is inconsistent about the case things in the Compatible ID string.
                        compatible_id.to_lowercase().starts_with(r"usb\class_fe&subclass_01")
                    },
                    None => true,
                }
        })
        .collect();

    // If there aren't any driverless Black Magic Probe DFU interfaces, then there's nothing to do.
    if devices.is_empty() {
        return;
    }

    println!("The libusb driver needs to be installed for the Black Magic Probe device before continuing. Standby...");
    std::thread::sleep(std::time::Duration::from_secs(1));

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
                Err(e) => {
                    // FIXME:
                    todo!("Create a log file!");
                }
            }
        }

        admin_install_drivers(&mut devices);

        // TODO: use the Windows SetupAPI to get the device instance ID of the BMP so we can restart it and re-enumerate it.
        // https://docs.microsoft.com/en-us/windows/win32/api/setupapi/nf-setupapi-setupdigetdeviceinstanceida

        // Now that we're done, nothing more to do in the admin process.
        std::process::exit(0);
    }

    // If we need to elevate, then we have to re-execute this process.

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
            warn!("Elevated process exited with {}; driver installation may not have succeeded", exit_code);
            return;
        } else {
            info!("Exiting parent process. Elevated process exited successfully.");
        }
    } else {
        Err::<(), _>(IoError::last_os_error())
            .expect("Error calling GetExitCodeProcess()");
    }

    println!(
        "Driver installation should be complete. \
        You may need to unplug the device and plug it back in before things work.."
    );
}
