use std::ffi::CString;
use std::mem::MaybeUninit;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::{Arc, Mutex};

use winapi::shared::minwindef::TRUE;
use winapi::shared::windef::HWND;
use winapi::shared::winerror::ERROR_ALREADY_EXISTS;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::knownfolders::FOLDERID_RoamingAppData;
use winapi::um::processthreadsapi::GetCurrentThreadId;
use winapi::um::shellapi::{NIM_DELETE, Shell_NotifyIconA};
use winapi::um::synchapi::CreateMutexA;
use winapi::um::wincon::FreeConsole;
use winapi::um::winuser::{DispatchMessageA, GetMessageA, IsDialogMessageA, MessageBoxA, MSG, TranslateMessage, WM_APP};
use wrapperrs::{Error, Result, ResultExt};

pub use confirmation::ask_confirmation;
use confirmation::show_dialog;
use pageant::listen_pageant;
use pipe::listen_named_pipe;
use taskbar::{base_icon_data, create_taskbar_icon};
use unix_socket::listen_unix_socket;
use utils::{check_error, format_error, get_known_folder};

use crate::agent::Agent;
use crate::NAME;

mod confirmation;
mod pageant;
mod pipe;
mod taskbar;
mod unix_socket;
mod utils;
mod process_describers;

const WM_SHOW_CONFIRMATION: u32 = WM_APP + 1;
const WM_NOTIFICATION_MSG: u32 = WM_APP + 2;
static mut MAIN_THREAD_ID: u32 = 0;

pub fn show_error(err: Box<dyn std::error::Error>) {
    let text = CString::new(format!("{}", err)).unwrap();
    unsafe {
        MessageBoxA(
            null_mut(),
            text.as_ptr(),
            "Error\0".as_ptr() as *const i8,
            0,
        )
    };
}

pub fn config_dir() -> PathBuf {
    get_known_folder(FOLDERID_RoamingAppData).join(NAME)
}

fn listener<F>(agent: &Arc<Mutex<Agent>>, name: &'static str, f: F)
    where
        F: FnOnce(Arc<Mutex<Agent>>) -> Result<()>,
        F: Send + 'static,
{
    let agent = agent.clone();
    std::thread::spawn(move || {
        if let Err(err) = f(agent).wrap_err(name) {
            show_error(err.into());
            std::process::exit(1);
        }
    });
}

pub fn serve(agent: Agent) -> Result<()> {
    unsafe {
        FreeConsole();

        let name = CString::new(NAME).unwrap();

        let mtx = CreateMutexA(null_mut(), TRUE, name.as_ptr());
        let err = GetLastError();
        if mtx == null_mut() {
            format_error(err)?;
        } else if err == ERROR_ALREADY_EXISTS {
            return Err(Error::new("Agent already running").into());
        }

        MAIN_THREAD_ID = GetCurrentThreadId();
        let agent = Arc::new(Mutex::new(agent));

        listener(&agent, "listen_named_pipe", listen_named_pipe);
        listener(&agent, "listen_pageant", listen_pageant);
        listener(&agent, "listen_unix_socket", listen_unix_socket);

        create_taskbar_icon(&agent)?;

        let mut msg: MSG = MaybeUninit::zeroed().assume_init();
        let mut dialog: HWND = null_mut();
        loop {
            let res = GetMessageA(&mut msg, null_mut(), 0, 0);
            if res == -1 {
                break;
            }
            if res == 0 {
                Shell_NotifyIconA(NIM_DELETE, &mut base_icon_data());
                break;
            }

            match msg.message.clone() {
                WM_SHOW_CONFIRMATION => {
                    dialog = show_dialog(msg.lParam)
                }
                _ if IsDialogMessageA(dialog, &mut msg) != 0 => {}
                _ => {
                    TranslateMessage(&msg);
                    DispatchMessageA(&msg);
                }
            }
        }
    }
    Ok(())
}
