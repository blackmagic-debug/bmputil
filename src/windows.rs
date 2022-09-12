use std::ptr;
use std::mem;
use std::env;
use std::iter;
use std::str::FromStr;
use std::io::Error as IoError;
use std::ffi::CString;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStrExt;

use libc::{intptr_t, c_int, c_uint, c_char, FILE, fileno, setvbuf, _IONBF};
use log::{info, warn};

use winapi::um::wincon::{FreeConsole, AttachConsole};
#[allow(unused_imports)]
use winapi::um::winbase::{STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE};
use winapi::um::consoleapi::AllocConsole;
use winapi::um::processenv::GetStdHandle;
use winapi::shared::minwindef::DWORD;
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


/// When our admin process is created, it does not inherit stdin, stdout, and stderr from the parent process.
/// AttachConsole(parent_pid) easily connects the admin process to the parent console, but, surprisingly
/// enough, that only restores stdio for *Rust*, and not C. How could this possibly be the case?
/// Well Rust's e.g. println!() eventually calls WinAPI's `WriteConsoleW()` using the console handle.
/// C's printf() on the other hand goes through whatever file descriptor the `stdout` global is set to,
/// and that is *not* updated when you call AttachConsole(). So, we need to resynchronize the
/// Microsoft C Runtime's stdio global state with the Windows console subsystem state.
pub fn restore_cstdio(parent_pid: u32) -> Result<(), IoError>
{
    unsafe {
        FreeConsole();
        if let Err(_e) = winapi_bool!(AttachConsole(parent_pid)) {
            // If we can't attach the previous console, then allocate a new console instead.
            // This will pop up a new window for the user, but that's better than no output
            // at all.
            AllocConsole();
        }
    }


    type ResourceGroup = (DWORD, CString, c_int, fn() -> *mut FILE);

    // Resync for each of stdin, stdout, and stderr.
    let stdio_resources: Vec<ResourceGroup> = vec![
        // FIXME: This function is supposed to restore stdin, stdout, and stderr, but _dup2 seems to fail for stdin.
        // I'm not certain why, but in the meantime, we'll skip restoring stdin since we don't need it.
        //(STD_INPUT_HANDLE, CString::new("r").unwrap(), STDIN_FILENO, stdinf),
        (STD_OUTPUT_HANDLE, CString::new("w").unwrap(), STDOUT_FILENO, stdoutf),
        (STD_ERROR_HANDLE, CString::new("w").unwrap(), STDERR_FILENO, stderrf),
    ];

    for resource_group in stdio_resources {
        let (std_handle, mode, std_fileno, std_file_fn) = resource_group;

        // Get the console subsystem handle attached to stdin/stdout/stderr.
        let win_handle = unsafe { winapi_handle!(GetStdHandle(std_handle)) }
            .expect("GetStdHandle() failed");

        // Then open that console subsystem handle as an Windows internal file descriptor
        // (not the same thing as the Unix-y file descriptor you get with `fileno()`).
        let win_fd = unsafe { winapi_neg!(_open_osfhandle(win_handle as intptr_t, _O_TEXT)) }
            .expect("_open_osfhandle() failed");

        // Now open that Windows internal file descriptor as a C FILE*.
        let c_stdio_file = unsafe { _fdopen(win_fd, mode.as_ptr()) };
        if c_stdio_file.is_null() {
            return Err::<(), _>(IoError::last_os_error());
        }

        // And finally, point stdout/stderr to the FILE* that we opened for this console.
        let _ = unsafe { winapi_neg!(_dup2(fileno(c_stdio_file), std_fileno)) }
            .expect("_dup2() failed");


        // Also, make stdio unbuffered.
        // That being said, setvbuf() only seems to succeed for stdout.
        // I can only assume that's because stdin isn't buffered in the first place, and stderr on Windows
        // uses the same console handle as stdout.
        // Either way, we're ignoring failures for this function call.
        let _ = unsafe { winapi_bool!(setvbuf(std_file_fn(), ptr::null_mut(), _IONBF, 0)) };

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
    let mut devices: Vec<_> = wdi::create_list(Default::default())
        .expect("Unable to get a list of connected devices")
        .into_iter()
        .filter(|d| {
            BmpVidPid::mode_from_vid_pid(Vid(d.vid), Pid(d.pid))
                .is_some()
                &&
                match d.compatible_id.as_ref() {
                    Some(compatible_id) => {
                        compatible_id.starts_with(r"USB\Class_fe&SubClass_01")
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
