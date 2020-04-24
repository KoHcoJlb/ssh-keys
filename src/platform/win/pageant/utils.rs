use std::ffi::c_void;
use std::ptr::null_mut;

use log::error;
use ntapi::ntexapi::{NtQuerySystemInformation, SYSTEM_HANDLE_INFORMATION_EX};
use ntapi::ntobapi::{NtQueryObject, OBJECT_INFORMATION_CLASS, OBJECT_NAME_INFORMATION};
use winapi::shared::minwindef::{FALSE, PULONG, ULONG};
use winapi::shared::ntdef::{HANDLE, NTSTATUS};
use winapi::shared::ntstatus::{STATUS_SUCCESS, STATUS_TIMEOUT};
use winapi::shared::winerror::WAIT_TIMEOUT;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::handleapi::DuplicateHandle;
use winapi::um::processthreadsapi::{
    CreateThread, GetCurrentProcess, GetCurrentProcessId, OpenProcess, TerminateThread,
};
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::winbase::WAIT_OBJECT_0;
use winapi::um::winnt::{PROCESS_DUP_HANDLE, PROCESS_QUERY_LIMITED_INFORMATION};

use super::super::utils::close_handle;

struct NtQueryObjectParams {
    handle: HANDLE,
    information_class: OBJECT_INFORMATION_CLASS,
    buffer: *mut c_void,
    buffer_size: ULONG,
    return_length: PULONG,
    status: NTSTATUS,
}

unsafe extern "system" fn nt_query_object_internal(params: *mut c_void) -> u32 {
    let params = &mut *(params as *mut NtQueryObjectParams);
    params.status = NtQueryObject(
        params.handle,
        params.information_class,
        params.buffer,
        params.buffer_size,
        params.return_length,
    );
    0
}

unsafe fn nt_query_object_threaded(
    handle: HANDLE,
    information_class: OBJECT_INFORMATION_CLASS,
    buffer: *mut c_void,
    buffer_size: ULONG,
    return_length: PULONG,
) -> NTSTATUS {
    let mut params = NtQueryObjectParams {
        handle,
        information_class,
        buffer,
        buffer_size,
        return_length,
        status: 0,
    };
    let thread = CreateThread(
        null_mut(),
        0,
        Some(nt_query_object_internal),
        &mut params as *mut _ as _,
        0,
        null_mut(),
    );

    let res = WaitForSingleObject(thread, 10);
    let err = GetLastError();
    if res == WAIT_TIMEOUT {
        TerminateThread(thread, 0);
        return STATUS_TIMEOUT;
    }

    if res == WAIT_OBJECT_0 {
        params.status
    } else {
        error!("nt_query_object_threaded: WaitForSingleObject res={}, err={}", res, err);
        TerminateThread(thread, 0);
        0x999
    }
}

pub unsafe fn find_memory_map_owner_process(mapping_name: &str) -> Option<u32> {
    let mut buffer = vec![0u8; 256 * 1024 * 1024];
    let status = NtQuerySystemInformation(
        0x40,
        buffer.as_mut_ptr() as _,
        buffer.len() as u32,
        null_mut(),
    );

    if status != STATUS_SUCCESS {
        return None;
    }

    let handle_info = &*(buffer.as_ptr() as *const SYSTEM_HANDLE_INFORMATION_EX);
    for handle in
    std::slice::from_raw_parts(handle_info.Handles.as_ptr(), handle_info.NumberOfHandles)
    {
        if handle.ObjectTypeIndex != 42 {
            continue;
        };
        if handle.UniqueProcessId == GetCurrentProcessId() as _ {
            continue;
        };

        let process = OpenProcess(PROCESS_DUP_HANDLE | PROCESS_QUERY_LIMITED_INFORMATION, FALSE, handle.UniqueProcessId as _);
        if process == null_mut() {
            continue;
        }
        let _close_process = close_handle(process);

        let mut dup_handle: HANDLE = null_mut();
        if DuplicateHandle(process, handle.HandleValue as _, GetCurrentProcess(), &mut dup_handle,
                           0, FALSE, 0, ) == 0 {
            continue;
        }
        let _close_dup_handle = close_handle(dup_handle);

        let mut buffer = [0u8; 1000];
        let status = nt_query_object_threaded(
            dup_handle,
            0x1,
            buffer.as_mut_ptr() as _,
            buffer.len() as _,
            null_mut(),
        );
        if status == STATUS_SUCCESS {
            let name = &(&*(buffer.as_ptr() as *const OBJECT_NAME_INFORMATION)).Name;
            let name = String::from_utf16(std::slice::from_raw_parts(
                name.Buffer,
                (name.Length / 2) as _,
            )).unwrap();
            if name.contains(mapping_name) {
                return Some(handle.UniqueProcessId as _);
            }
        }
    }
    None
}
