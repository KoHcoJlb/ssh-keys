use std::cmp::{max, min};
use std::collections::HashMap;
use std::ffi::CString;
use std::mem::{size_of, size_of_val, transmute};
use std::ptr::{null, null_mut};
use std::sync::mpsc::{channel, Sender};

use winapi::_core::mem::MaybeUninit;
use winapi::shared::minwindef::{FALSE, LPARAM, LRESULT, TRUE, UINT, WPARAM};
use winapi::shared::windef::{HDC, HWND, POINT, RECT, SIZE};
use winapi::um::commctrl::{TOOLINFOW, TOOLTIPS_CLASS, TTDT_AUTOPOP, TTM_ADDTOOLW, TTM_RELAYEVENT,
                           TTM_SETDELAYTIME, TTM_SETMAXTIPWIDTH, TTS_ALWAYSTIP};
use winapi::um::libloaderapi::GetModuleHandleA;
use winapi::um::processthreadsapi::GetCurrentThreadId;
use winapi::um::wingdi::{CLIP_DEFAULT_PRECIS, CreateFontA, DEFAULT_CHARSET, DEFAULT_PITCH, DEFAULT_QUALITY, DeleteObject, GetTextExtentPoint32W, OUT_DEFAULT_PRECIS, SelectObject};
use winapi::um::winuser::{AttachThreadInput, BN_CLICKED, CreateDialogParamA, CreateWindowExA, CW_USEDEFAULT, DestroyWindow, DLGPROC, GetClientRect, GetDC, GetDlgItem, GetForegroundWindow, GetMonitorInfoA, GetWindowLongPtrA, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, GWL_USERDATA, HWND_TOP, IDCANCEL, IDOK, MAKEINTRESOURCEA, MapDialogRect, MapWindowPoints, MONITOR_DEFAULTTONULL, MonitorFromWindow, MONITORINFO, MSG, PostThreadMessageA, SendMessageW, SetDlgItemTextW, SetFocus, SetForegroundWindow, SetWindowLongPtrA, SetWindowPos, ShowWindow, SW_SHOW, SWP_NOSIZE, WM_COMMAND, WM_CTLCOLORSTATIC, WM_DESTROY, WM_INITDIALOG, WS_POPUP};

use crate::agent::{RequesterInfo, RequestInfo};
use crate::config::Config;
use crate::key::KeyPair;
use crate::utils::Finally;

use super::{MAIN_THREAD_ID, WM_SHOW_CONFIRMATION};
use super::utils::StrExt;

const DWLP_USER: i32 = (size_of::<LRESULT>() + size_of::<DLGPROC>()) as i32;

unsafe fn bring_to_front(window: HWND) {
    let f_wnd = GetForegroundWindow();
    let c_thd = GetCurrentThreadId();
    let f_thd = GetWindowThreadProcessId(f_wnd, null_mut());

    AttachThreadInput(f_thd, c_thd, TRUE);
    SetForegroundWindow(window);
    AttachThreadInput(f_thd, c_thd, FALSE);
}

struct Confirmation<'a> {
    key_pair: &'a KeyPair,
    sender: Sender<bool>,
    req_info: &'a RequestInfo,
    tooltip: Option<HWND>,
    dlg: Option<HWND>,
}

pub unsafe extern "system" fn dlg_proc(
    dlg: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe fn map_width(dlg: HWND, width: i32) -> i32 {
        let mut rect = RECT {
            left: 0,
            right: width,
            top: 0,
            bottom: 0,
        };
        MapDialogRect(dlg, &mut rect);
        rect.right
    }

    // Forward window messages to tooltip
    {
        let ptr = GetWindowLongPtrA(dlg, DWLP_USER) as *const Confirmation;
        if !ptr.is_null() {
            let confirmation = &*ptr;
            if let Some(tooltip) = confirmation.tooltip {
                let msg = MSG {
                    hwnd: dlg,
                    lParam: lparam,
                    wParam: wparam,
                    message: msg,
                    pt: POINT {
                        x: 0,
                        y: 0,
                    },
                    time: 0,
                };
                SendMessageW(tooltip, TTM_RELAYEVENT, 0, &msg as *const _ as _);
            }
        }
    }

    match msg {
        WM_INITDIALOG => {
            let confirmation = &mut *(lparam as *mut Confirmation);
            SetWindowLongPtrA(dlg, DWLP_USER, lparam);
            confirmation.dlg = Some(dlg);

            bring_to_front(dlg);
            SetFocus(GetDlgItem(dlg, IDCANCEL));

            let (description_short, description_long) =
                if let Some(RequesterInfo {
                                description_short,
                                description_long
                            }) = &confirmation.req_info.requester {
                    (description_short.as_str(), Some(description_long.as_str()))
                } else { ("Unknown", None) };
            SetDlgItemTextW(dlg, 4, description_short.to_utf16_null().as_ptr());
            SetDlgItemTextW(dlg, 6, confirmation.key_pair.name().to_utf16_null().as_ptr());
            SetDlgItemTextW(dlg, 8, confirmation.req_info.channel.to_utf16_null().as_ptr());

            let text_controls = 4..=8;

            // Create font for text controls, calculate max width
            let text_width: HashMap<_, _> = text_controls
                .clone()
                .map(|control_id| {
                    let control = GetDlgItem(dlg, control_id);
                    let font_weight = match control_id {
                        4 | 6 | 8 => 700,
                        _ => 300
                    };
                    let font = CreateFontA(19, 0, 0, 0, font_weight, 0, 0, 0,
                                           DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                                           DEFAULT_QUALITY, DEFAULT_PITCH, "Arial\0".as_ptr() as _);
                    SetWindowLongPtrA(control, GWL_USERDATA, font as _);

                    let mut text = vec![0; GetWindowTextLengthW(control) as _];
                    GetWindowTextW(control, text.as_mut_ptr(), text.len() as _);

                    let hdc = GetDC(control);
                    SelectObject(hdc, font as _);
                    let mut size: SIZE = MaybeUninit::zeroed().assume_init();
                    GetTextExtentPoint32W(hdc, text.as_ptr(), text.len() as _, &mut size);
                    (control_id, size.cx)
                })
                .collect();

            // Get monitor size
            let monitor = MonitorFromWindow(dlg, MONITOR_DEFAULTTONULL);
            let mut monitor_info: MONITORINFO = MaybeUninit::zeroed().assume_init();
            monitor_info.cbSize = size_of_val(&monitor_info) as _;
            GetMonitorInfoA(monitor, &mut monitor_info);
            let monitor_size = monitor_info.rcWork;

            let max_text_width = (monitor_size.right - monitor_size.left) / 100 * 60;
            let max_text_width = min(max_text_width, max(map_width(dlg, 160),
                                                         *text_width.values().max().unwrap()));

            // Calculate dialog borders
            let mut rc_client: RECT = MaybeUninit::zeroed().assume_init();
            let mut rc_wind: RECT = MaybeUninit::zeroed().assume_init();
            GetClientRect(dlg, &mut rc_client);
            GetWindowRect(dlg, &mut rc_wind);
            let border_x = (rc_wind.right - rc_wind.left) - rc_client.right;

            // Calculate new dialog's width
            let min_width = map_width(dlg, 180);
            let max_width = (monitor_size.right - monitor_size.left) / 100 * 60;
            let dlg_new_width = min(max_width, max(min_width,
                                                   max_text_width + map_width(dlg, 20) + border_x));
            let dlg_width_diff = dlg_new_width - (rc_wind.right - rc_wind.left);

            // Set new width and recenter dialog
            let mut rect: RECT = MaybeUninit::zeroed().assume_init();
            GetWindowRect(dlg, &mut rect);
            SetWindowPos(dlg, HWND_TOP,
                         ((monitor_size.right - monitor_size.left) - dlg_new_width) / 2,
                         ((monitor_size.bottom - monitor_size.top) -
                             (rc_wind.bottom - rc_wind.top)) / 2,
                         dlg_new_width,
                         rect.bottom - rect.top,
                         0);

            // Set new width for text controls
            for control_id in text_controls.clone() {
                let control = GetDlgItem(dlg, control_id);
                let mut rect: RECT = MaybeUninit::zeroed().assume_init();
                GetWindowRect(control, &mut rect);
                MapWindowPoints(null_mut(), dlg, &mut rect as *mut _ as _, 2);

                let width = text_width[&control_id];
                SetWindowPos(control, HWND_TOP,
                             (dlg_new_width - width) / 2,
                             rect.top,
                             width,
                             rect.bottom - rect.top,
                             0);
            }

            // Move buttons to match new dialog width
            for &control_id in [IDCANCEL, IDOK].iter() {
                let control = GetDlgItem(dlg, control_id);
                let mut rect: RECT = MaybeUninit::zeroed().assume_init();
                GetWindowRect(control, &mut rect);
                MapWindowPoints(null_mut(), dlg, &mut rect as *mut _ as _, 2);
                SetWindowPos(control, HWND_TOP, rect.left + dlg_width_diff, rect.top, 0, 0,
                             SWP_NOSIZE);
            }

            // Create tooltip with long description
            if let Some(description_long) = description_long {
                const CONTROL_ID: i32 = 4;
                let control = GetDlgItem(dlg, CONTROL_ID);

                let mut description_long = description_long.to_utf16_null();

                let class = CString::new(TOOLTIPS_CLASS).unwrap();
                let tooltip = CreateWindowExA(0, class.as_ptr(),
                                              null(), WS_POPUP | TTS_ALWAYSTIP,
                                              CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT,
                                              CW_USEDEFAULT, dlg, null_mut(),
                                              GetModuleHandleA(null_mut()), null_mut());

                let mut tool_info: TOOLINFOW = MaybeUninit::zeroed().assume_init();
                tool_info.cbSize = size_of::<TOOLINFOW>() as _;
                tool_info.hwnd = dlg;
                tool_info.lpszText = description_long.as_mut_ptr();

                GetWindowRect(control, &mut tool_info.rect);
                MapWindowPoints(null_mut(), dlg, &mut tool_info.rect as *mut _ as _, 2);

                SendMessageW(tooltip, TTM_ADDTOOLW, 0, &mut tool_info as *mut _ as _);
                SendMessageW(tooltip, TTM_SETMAXTIPWIDTH, 0, dlg_new_width as _);
                SendMessageW(tooltip, TTM_SETDELAYTIME, TTDT_AUTOPOP, 0x7fff);

                confirmation.tooltip = Some(tooltip);
            }

            0
        }
        WM_COMMAND => {
            let n_code = ((wparam >> 16) & 0xffff) as u16;
            let btn_id = (wparam & 0xffff) as i32;
            if n_code == BN_CLICKED {
                let confirmation: &Confirmation = transmute(GetWindowLongPtrA(dlg, DWLP_USER));
                confirmation.sender.send(btn_id == IDOK).unwrap();
                DestroyWindow(dlg);
            }
            1
        }
        WM_CTLCOLORSTATIC => {
            let hdc = wparam as HDC;
            let control = lparam as HWND;

            SelectObject(hdc, GetWindowLongPtrA(control, GWL_USERDATA) as _);
            0
        }
        WM_DESTROY => {
            for control_id in 4..=8 {
                let control = GetDlgItem(dlg, control_id);
                let font = GetWindowLongPtrA(control, GWL_USERDATA);
                DeleteObject(font as _);
            }

            1
        }
        _ => 0,
    }
}

pub unsafe fn show_dialog(param: LPARAM) -> HWND {
    let dialog = CreateDialogParamA(
        null_mut(),
        MAKEINTRESOURCEA(101),
        null_mut(),
        Some(dlg_proc),
        param,
    );
    ShowWindow(dialog, SW_SHOW);
    dialog
}

pub fn ask_confirmation(key_pair: &KeyPair, req_info: &RequestInfo, _config: &Config) -> bool {
    unsafe {
        let (sender, receiver) = channel::<bool>();
        let confirmation = Confirmation { key_pair, sender, req_info, tooltip: None, dlg: None };
        let _set_null = Finally::new(|| {
            if let Some(dlg) = confirmation.dlg {
                SetWindowLongPtrA(dlg, DWLP_USER, 0);
            }
        });

        PostThreadMessageA(MAIN_THREAD_ID, WM_SHOW_CONFIRMATION, 0, &confirmation as *const _ as _);

        receiver.recv().unwrap()
    }
}
