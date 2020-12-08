use std::ffi::{c_void, CStr};
use std::mem::{MaybeUninit, size_of_val, transmute};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use itertools::Itertools;
use log::error;
use ntapi::ntpebteb::PEB;
use ntapi::ntpsapi::{NtQueryInformationProcess, PROCESS_BASIC_INFORMATION, ProcessBasicInformation};
use ntapi::ntrtl::RTL_USER_PROCESS_PARAMETERS;
use winapi::shared::guiddef::GUID;
use winapi::shared::minwindef::{BOOL, DWORD, FALSE, LPARAM, TRUE};
use winapi::shared::ntdef::{HANDLE, UNICODE_STRING};
use winapi::shared::ntstatus::STATUS_SUCCESS;
use winapi::shared::windef::HWND;
use winapi::um::combaseapi::CoTaskMemFree;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::handleapi::CloseHandle;
use winapi::um::memoryapi::ReadProcessMemory;
use winapi::um::processthreadsapi::OpenProcess;
use winapi::um::psapi::GetModuleFileNameExW;
use winapi::um::shlobj::SHGetKnownFolderPath;
use winapi::um::winbase::{FORMAT_MESSAGE_ALLOCATE_BUFFER, FORMAT_MESSAGE_FROM_SYSTEM, FormatMessageA, LocalFree};
use winapi::um::winnt::{LANG_USER_DEFAULT, LPSTR, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ, PWSTR};
use winapi::um::winuser::{EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};
use winapi::um::winver::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use wrapperrs::Error;

use crate::agent::RequesterInfo;
use crate::config::Config;
use crate::utils::Finally;

use super::process_describers::describe;

pub trait StrExt {
    fn to_utf16_null(&self) -> Vec<u16>;
}

impl StrExt for &str {
    fn to_utf16_null(&self) -> Vec<u16> {
        let mut v: Vec<_> = self.encode_utf16().collect();
        v.push(0);
        v
    }
}

pub fn check_error() -> wrapperrs::Result<()> {
    format_error(unsafe { GetLastError() })
}

pub fn format_error(err: u32) -> wrapperrs::Result<()> {
    unsafe {
        if err == 0 {
            return Ok(());
        }

        let msg_ptr: LPSTR = null_mut();
        FormatMessageA(
            FORMAT_MESSAGE_ALLOCATE_BUFFER | FORMAT_MESSAGE_FROM_SYSTEM,
            null(),
            err as u32,
            LANG_USER_DEFAULT as u32,
            transmute(&msg_ptr),
            0,
            null_mut(),
        );

        let msg = CStr::from_ptr(msg_ptr).to_str().unwrap();
        let err = wrapperrs::Error::new(&format!("(win32) {}", &msg[..msg.len() - 2]));
        LocalFree(msg_ptr as *mut c_void);
        Err(err.into())
    }
}

pub unsafe fn close_handle(handle: *mut c_void) -> impl Drop {
    Finally::new(move || { CloseHandle(handle); })
}

pub fn get_known_folder(folder_id: GUID) -> PathBuf {
    unsafe {
        let mut wstr: PWSTR = null_mut();
        SHGetKnownFolderPath(&folder_id, 0, null_mut(), &mut wstr);

        let length = (0..).into_iter()
            .take_while(|i| wstr.offset(*i).read() != 0)
            .count();
        let str = String::from_utf16(
            std::slice::from_raw_parts(wstr, length)).unwrap();
        CoTaskMemFree(wstr as *mut c_void);

        PathBuf::from(str)
    }
}

pub unsafe fn get_executable_from_pid(pid: u32) -> wrapperrs::Result<PathBuf> {
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    if process == null_mut() {
        return Err(Error::new("OpenProcess").into());
    };
    let _close_process = close_handle(process);

    let mut name = [0u16; 32 * 1024];
    let length = GetModuleFileNameExW(process, null_mut(), name.as_mut_ptr(), name.len() as _);
    if length == 0 {
        Err(Error::new("GetModuleFileNameExW").into())
    } else {
        Ok(PathBuf::from(String::from_utf16(&name[..length as _]).unwrap()))
    }
}

pub unsafe fn get_executable_description(exe: &Path) -> Result<String, ()> {
    let exe_utf16 = exe.to_str().unwrap().to_utf16_null();

    let mut handle: DWORD = 0;
    let size = GetFileVersionInfoSizeW(exe_utf16.as_ptr(), &mut handle);
    if size == 0 {
        error!("GetFileVersionInfoSizeW, err={}, exe={}", GetLastError(), exe.to_str().unwrap());
        return Err(());
    }

    let mut data = vec![0u8; size as _];
    if GetFileVersionInfoW(exe_utf16.as_ptr(), 0, data.len() as _,
                           data.as_mut_ptr() as _) == 0 {
        error!("GetFileVersionInfoW, err={}, exe={}", GetLastError(), exe.to_str().unwrap());
        return Err(());
    }

    let mut data_ptr: *mut DWORD = null_mut();
    let mut size: u32 = 0;
    if VerQueryValueW(data.as_ptr() as _,
                      r"\VarFileInfo\Translation".to_utf16_null().as_ptr(),
                      &mut *(&mut data_ptr as *mut _ as *mut *mut _), &mut size as _) == 0 {
        error!("VerQueryValueW (translation), err={}, exe={}", GetLastError(), exe.to_str().unwrap());
        return Err(());
    }

    let language = *data_ptr;
    let lang_id = language & 0xffff;
    let code_page = language >> 16 & 0xffff;

    let mut data_ptr: *mut u16 = null_mut();
    let mut size: u32 = 0;
    let query = format!(r"\StringFileInfo\{:0>4x}{:0>4x}\FileDescription", lang_id, code_page);
    if VerQueryValueW(data.as_ptr() as _, query.as_str().to_utf16_null().as_ptr(),
                      &mut *(&mut data_ptr as *mut _ as *mut *mut _),
                      &mut size as _) == 0 {
        let err = GetLastError();
        // 1813 - FileDescription resource type not found
        if err != 1813 {
            error!("VerQueryValueW (file description), err={}, exe={}, query={}", err,
                   exe.to_str().unwrap(), query);
        }
        return Err(());
    };

    let data: Vec<_> = (0..).step_by(2)
        .map(|offset| data_ptr.offset(offset / 2).read())
        .take_while(|c| *c != 0)
        .collect();
    Ok(String::from_utf16(&data).unwrap())
}

pub unsafe fn get_parent_pid(pid: u32) -> u32 {
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    if process == null_mut() {
        return 0;
    }
    let _close_process = close_handle(process);

    let mut info: PROCESS_BASIC_INFORMATION = MaybeUninit::zeroed().assume_init();
    if NtQueryInformationProcess(process, ProcessBasicInformation, &mut info as *mut _ as _,
                                 size_of_val(&info) as _, null_mut()) != STATUS_SUCCESS {
        return 0;
    }

    info.InheritedFromUniqueProcessId as _
}

pub unsafe fn find_primary_window(process_id: u32) -> Option<HWND> {
    struct Data {
        process_id: u32,
        windows: Vec<HWND>,
    }

    unsafe extern "system" fn window_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam as *mut Data);

        let mut process_id = 0;
        GetWindowThreadProcessId(hwnd, &mut process_id);
        if process_id == data.process_id {
            data.windows.push(hwnd);
        };
        TRUE
    }

    let mut data = Data {
        process_id,
        windows: Vec::new(),
    };
    EnumWindows(Some(window_proc), &mut data as *mut _ as _);

    if data.windows.is_empty() {
        return None;
    };

    data.windows
        .iter()
        .find(|&&hwnd| IsWindowVisible(hwnd) == TRUE)
        .or_else(|| data.windows.first())
        .copied()
}

pub unsafe fn get_window_text(win: HWND) -> Result<String, ()> {
    let mut title = vec![0; (GetWindowTextLengthW(win) + 1) as _];
    let length = GetWindowTextW(win, title.as_mut_ptr(), title.len() as _);
    if length > 0 {
        Ok(String::from_utf16(&title[..length as _]).unwrap())
    } else {
        Err(())
    }
}

pub unsafe fn get_process_command_line(pid: u32) -> Result<String, ()> {
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
                              FALSE, pid);
    if process == null_mut() {
        return Err(());
    }
    let _close_process = close_handle(process);

    let mut info: PROCESS_BASIC_INFORMATION = MaybeUninit::zeroed().assume_init();
    let res = NtQueryInformationProcess(process, ProcessBasicInformation,
                                        &mut info as *mut _ as _,
                                        size_of_val(&info) as u32, null_mut());
    if res != STATUS_SUCCESS {
        return Err(());
    }

    unsafe fn read_process<T>(process: HANDLE, addr: *mut c_void) -> std::result::Result<T, ()> {
        let mut dst: T = MaybeUninit::zeroed().assume_init();
        if ReadProcessMemory(process, addr, &mut dst as *mut _ as _, size_of_val(&dst),
                             null_mut()) == 0 {
            dbg!(GetLastError());
            Err(())
        } else {
            Ok(dst)
        }
    }

    unsafe fn read_process_unicode_string(process: HANDLE, s: UNICODE_STRING)
        -> std::result::Result<String, ()> {
        let mut buffer = vec![0u16; (s.Length / 2) as _];
        if ReadProcessMemory(process, s.Buffer as _, buffer.as_mut_ptr() as _,
                             s.Length as _, null_mut()) == 0 {
            dbg!(GetLastError());
            return Err(());
        }
        Ok(String::from_utf16(&buffer).unwrap())
    }

    if let Ok(command_line) = (|| -> std::result::Result<_, ()> {
        let peb: PEB = read_process(process, info.PebBaseAddress as _)?;
        let parameters: RTL_USER_PROCESS_PARAMETERS = read_process(process,
                                                                   peb.ProcessParameters as _)?;
        read_process_unicode_string(process, parameters.CommandLine)
    })() {
        Ok(command_line)
    } else {
        Err(())
    }
}

pub unsafe fn collect_requester_info(_config: &Config, mut pid: u32)
    -> wrapperrs::Result<RequesterInfo> {
    let mut process_stack = Vec::new();
    while pid != 0 {
        let window = find_primary_window(pid);
        process_stack.push((pid, window));
        pid = get_parent_pid(pid);
    }

    let main_process = process_stack.iter()
        .find(|(_, window)| match *window {
            Some(window) if IsWindowVisible(window) == TRUE => true,
            _ => false
        })
        .or_else(|| process_stack.iter()
            .find(|(_, window)| window.is_some()))
        .or(process_stack.first())
        .unwrap();

    let short = describe(main_process.0, main_process.1)?.0;
    let long = process_stack
        .iter()
        .filter_map(|(pid, window)| describe(*pid, *window)
            .map(|(_, long)| long)
            .ok())
        .intersperse("\n\n".into())
        .collect::<String>();

    Ok(RequesterInfo {
        description_short: short,
        description_long: long,
    })
}
