use std::ptr::null_mut;

use log::trace;
use ntapi::ntexapi::{NtQuerySystemInformation, SYSTEM_HANDLE_INFORMATION_EX};
use ntapi::ntobapi::{NtQueryObject, OBJECT_NAME_INFORMATION, ObjectNameInformation};
use winapi::shared::minwindef::FALSE;
use winapi::shared::ntdef::HANDLE;
use winapi::shared::ntstatus::STATUS_SUCCESS;
use winapi::um::handleapi::DuplicateHandle;
use winapi::um::processthreadsapi::{GetCurrentProcess, GetCurrentProcessId, OpenProcess};
use winapi::um::winnt::{PROCESS_DUP_HANDLE, PROCESS_QUERY_LIMITED_INFORMATION};

use super::super::utils::close_handle;

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
    let handles = std::slice::from_raw_parts(
        handle_info.Handles.as_ptr(), handle_info.NumberOfHandles);
    for handle in handles {
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
        let status = NtQueryObject(dup_handle, ObjectNameInformation, buffer.as_mut_ptr() as _,
                                   buffer.len() as _, null_mut());
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
