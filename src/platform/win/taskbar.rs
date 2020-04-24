use std::ffi::CString;
use std::mem::{MaybeUninit, size_of, size_of_val};
use std::ptr::{copy_nonoverlapping, null_mut};
use std::sync::{Arc, Mutex};

use lazy_static::lazy_static;
use openssl::hash::{hash, MessageDigest};
use winapi::shared::guiddef::GUID;
use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::HWND;
use winapi::um::libloaderapi::GetModuleHandleA;
use winapi::um::shellapi::{
    NIF_GUID, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAA, Shell_NotifyIconA,
};
use winapi::um::winuser::{
    CreateWindowExA, DefWindowProcA, GetClassLongPtrA, GetSubMenu, HWND_MESSAGE, LoadIconA,
    LoadMenuA, MAKEINTRESOURCEA, PostQuitMessage, RegisterClassA, SetClassLongPtrA,
    SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_VERPOSANIMATION, TrackPopupMenuEx,
    WM_CONTEXTMENU, WNDCLASSA,
};
use wrapperrs::{Result, ResultExt};

use crate::agent::Agent;
use crate::NAME;
use crate::platform::show_error;

use super::utils::check_error;
use super::WM_NOTIFICATION_MSG;

lazy_static! {
    static ref ICON_GUID: GUID = {
        let exe_path = std::env::current_exe().unwrap().canonicalize().unwrap();
        let digest = hash(MessageDigest::md5(), exe_path.to_str().unwrap().as_bytes()).unwrap();
        unsafe { *(digest.as_ptr() as *const GUID) }
    };
}

pub fn base_icon_data() -> NOTIFYICONDATAA {
    let mut icon_data: NOTIFYICONDATAA = unsafe { MaybeUninit::zeroed().assume_init() };
    icon_data.cbSize = size_of_val(&icon_data) as u32;
    icon_data.uFlags = NIF_GUID;
    icon_data.guidItem = *ICON_GUID;
    icon_data
}

unsafe extern "system" fn wnd_proc(
    window: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NOTIFICATION_MSG => {
            let msg = (lparam & 0xffff) as u32;
            if msg == WM_CONTEXTMENU {
                let menu = LoadMenuA(GetModuleHandleA(null_mut()), MAKEINTRESOURCEA(201));
                let menu = GetSubMenu(menu, 0);

                let agent = &*(GetClassLongPtrA(window, 0) as *const Arc<Mutex<Agent>>);

                let x = (wparam & 0xffff) as i32;
                let y = ((wparam >> 16) & 0xffff) as i32;
                SetForegroundWindow(window);
                match TrackPopupMenuEx(
                    menu,
                    TPM_VERPOSANIMATION | TPM_NONOTIFY | TPM_RETURNCMD,
                    x,
                    y,
                    window,
                    null_mut(),
                ) {
                    1 => PostQuitMessage(0),
                    2 => {
                        let mut lock = agent.lock().unwrap();
                        if let Err(err) = lock.config_mut().reload().wrap_err("reload config") {
                            show_error(err.into());
                        }
                    }
                    _ => {}
                };
            };
            1
        }
        _ => DefWindowProcA(window, msg, wparam, lparam),
    }
}

pub fn create_taskbar_icon(agent: &Arc<Mutex<Agent>>) -> Result<()> {
    unsafe {
        let class_name = CString::new(NAME).unwrap();

        let mut wndclass: WNDCLASSA = MaybeUninit::zeroed().assume_init();
        wndclass.lpfnWndProc = Some(wnd_proc);
        wndclass.lpszClassName = class_name.as_ptr();
        wndclass.cbClsExtra = size_of::<*const Arc<Mutex<Agent>>>() as i32;
        if RegisterClassA(&wndclass) == 0 {
            check_error().wrap_err("RegisterClassA")?;
        }

        let window = CreateWindowExA(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
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

        SetClassLongPtrA(window, 0, agent as *const _ as isize);

        let mut icon_data = base_icon_data();
        icon_data.hWnd = window;
        icon_data.uFlags |= NIF_TIP | NIF_SHOWTIP | NIF_MESSAGE | NIF_ICON;
        *icon_data.u.uVersion_mut() = NOTIFYICON_VERSION_4;
        icon_data.uCallbackMessage = WM_NOTIFICATION_MSG;
        icon_data.hIcon = LoadIconA(GetModuleHandleA(null_mut()), MAKEINTRESOURCEA(2));

        const TOOLTIP: &str = "ssh-agent\0";
        copy_nonoverlapping(
            TOOLTIP.as_ptr() as *const i8,
            icon_data.szTip.as_mut_ptr(),
            TOOLTIP.len(),
        );

        Shell_NotifyIconA(NIM_DELETE, &mut icon_data);
        Shell_NotifyIconA(NIM_ADD, &mut icon_data);
        Shell_NotifyIconA(NIM_SETVERSION, &mut icon_data);
    };
    Ok(())
}
