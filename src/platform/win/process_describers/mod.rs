use winapi::shared::windef::HWND;

use super::utils::get_executable_from_pid;

mod default;

pub fn describe(pid: u32, window: Option<HWND>) -> wrapperrs::Result<(String, String)> {
    unsafe {
        let exe = get_executable_from_pid(pid)?;
        match exe.file_name().unwrap().to_str().unwrap() {
            _ => default::describe(pid, window)
        }.map(|mut desc| {
            desc.1.insert_str(0, &format!("{} : ", pid));
            desc
        })
    }
}
