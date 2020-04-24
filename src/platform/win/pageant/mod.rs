use std::ffi::CStr;
use std::io::{Cursor, Read, Write};
use std::mem::{MaybeUninit, transmute};
use std::ptr::null_mut;
use std::sync::{Arc, Mutex};

use log::error;
use log::trace;
use winapi::_core::intrinsics::copy_nonoverlapping;
use winapi::shared::minwindef::{FALSE, LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::HWND;
use winapi::um::handleapi::CloseHandle;
use winapi::um::memoryapi::{FILE_MAP_WRITE, MapViewOfFile, UnmapViewOfFile, VirtualQuery};
use winapi::um::winbase::OpenFileMappingA;
use winapi::um::winnt::MEMORY_BASIC_INFORMATION;
use winapi::um::winuser::{CreateWindowExA, DefWindowProcA, DispatchMessageA, FindWindowA, GetClassLongPtrA, GetMessageA, HWND_MESSAGE, MSG, PCOPYDATASTRUCT, RegisterClassA, SetClassLongPtrA, TranslateMessage, WM_COPYDATA, WNDCLASSA, WS_CAPTION};
use wrapperrs::{Error, Result, ResultExt};

use utils::find_memory_map_owner_process;

use crate::agent::{Agent, RequestInfo};
use crate::utils::{connection_handler, ReadWrite};

use super::check_error;
use super::utils::collect_requester_info;

mod utils;

const CLASS_NAME: *const i8 = "Pageant\0".as_ptr() as *const i8;

struct BufReadWrite<'a> {
    req: Cursor<&'a [u8]>,
    resp: &'a mut Vec<u8>,
}

impl<'a> ReadWrite for BufReadWrite<'a> {
    fn read(&mut self) -> &mut dyn Read {
        &mut self.req
    }

    fn write(&mut self) -> &mut dyn Write {
        &mut self.resp
    }
}

extern "system" fn wnd_proc(window: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_COPYDATA {
            let copydata = *transmute::<_, PCOPYDATASTRUCT>(lparam);
            let mapping_name = CStr::from_ptr(copydata.lpData as *const i8)
                .to_str()
                .unwrap();
            trace!("pageant request {}", mapping_name);

            let mapping_file =
                OpenFileMappingA(FILE_MAP_WRITE, FALSE, copydata.lpData as *const i8);
            let mapping_ptr = MapViewOfFile(mapping_file, FILE_MAP_WRITE, 0, 0, 0);
            let mut mapping_info: MEMORY_BASIC_INFORMATION = MaybeUninit::zeroed().assume_init();
            VirtualQuery(
                mapping_ptr,
                &mut mapping_info,
                std::mem::size_of_val(&mapping_info),
            );
            let mapping =
                std::slice::from_raw_parts_mut(mapping_ptr as *mut u8, mapping_info.RegionSize);

            let agent = &*(GetClassLongPtrA(window, 0) as *mut Arc<Mutex<Agent>>);
            let agent = agent.clone();

            let requester = find_memory_map_owner_process(mapping_name)
                .and_then(|process_id| {
                    let agent = agent.lock().unwrap();
                    collect_requester_info(agent.config(), process_id).ok()
                });

            let info = RequestInfo {
                channel: "Pageant",
                requester,
            };

            let mut resp = Vec::new();
            connection_handler(agent, &mut BufReadWrite {
                req: Cursor::new(mapping),
                resp: &mut resp,
            }, info);
            if resp.len() > mapping.len() {
                error!("resp len > req len");
            } else {
                copy_nonoverlapping(resp.as_ptr(), mapping.as_mut_ptr(), resp.len());
            }

            UnmapViewOfFile(mapping_ptr);
            CloseHandle(mapping_file);

            1
        } else {
            DefWindowProcA(window, msg, wparam, lparam)
        }
    }
}

pub fn listen_pageant(agent: Arc<Mutex<Agent>>) -> Result<()> {
    unsafe {
        if FindWindowA(CLASS_NAME, CLASS_NAME) != null_mut() {
            return Err(Error::new("Pageant already running").into());
        }

        let mut wndclass: WNDCLASSA = MaybeUninit::zeroed().assume_init();
        wndclass.lpfnWndProc = Some(wnd_proc);
        wndclass.lpszClassName = CLASS_NAME;
        wndclass.cbClsExtra = std::mem::size_of::<*const Arc<Mutex<Agent>>>() as i32;
        if RegisterClassA(&wndclass) == 0 {
            check_error().wrap_err("RegisterClassA")?;
        }

        let window = CreateWindowExA(
            0,
            CLASS_NAME,
            CLASS_NAME,
            WS_CAPTION,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            null_mut(),
            null_mut(),
        );
        if window.is_null() {
            check_error().wrap_err("CreateWindowExA")?;
        }
        SetClassLongPtrA(window, 0, &agent as *const _ as isize);

        let mut msg: MSG = MaybeUninit::zeroed().assume_init();
        loop {
            let res = GetMessageA(&mut msg, null_mut(), 0, 0);
            if res == 0 || res == -1 {
                break;
            }

            TranslateMessage(&msg);
            DispatchMessageA(&msg);
        }
        Ok(())
    }
}
