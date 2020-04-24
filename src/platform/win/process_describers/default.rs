use winapi::shared::windef::HWND;
use wrapperrs::{Result, ResultExt};

use super::super::utils::{get_executable_description, get_executable_from_pid,
                          get_process_command_line, get_window_text};

pub fn describe(pid: u32, window: Option<HWND>) -> Result<(String, String)> {
    unsafe {
        let exe = get_executable_from_pid(pid)
            .wrap_err(&format!("get_executable_from_pid pid={}", pid))?;

        let mut process_name = exe.file_name().unwrap().to_str().unwrap().to_string();

        if let Ok(description) = get_executable_description(&exe) {
            process_name.push_str(&format!(" - {}", description));
        }

        if let Ok(text) = window.ok_or(())
            .and_then(|hwnd| get_window_text(hwnd)) {
            process_name.push_str(&format!(" - {}", text));
        }

        let mut long = get_process_command_line(pid)
            .unwrap_or(exe.to_str().unwrap().to_string());
        long.insert_str(0, &format!("{} : ", process_name));

        Ok((process_name, long))
    }
}
