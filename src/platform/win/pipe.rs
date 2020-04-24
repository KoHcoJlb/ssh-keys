use std::io;
use std::io::{Read, Write};
use std::ptr::null_mut;
use std::sync::{Arc, Mutex};

use winapi::ctypes::c_void;
use winapi::shared::minwindef::ULONG;
use winapi::um::fileapi::{FlushFileBuffers, ReadFile, WriteFile};
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::namedpipeapi::{ConnectNamedPipe, DisconnectNamedPipe};
use winapi::um::winbase::{CreateNamedPipeA, GetNamedPipeClientProcessId, PIPE_ACCESS_DUPLEX, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT};
use winapi::um::winnt::HANDLE;
use wrapperrs::Result;

use crate::agent::{Agent, RequestInfo};
use crate::utils::{connection_handler, ReadWrite};

use super::check_error;
use super::utils::collect_requester_info;

const PIPE_NAME: *const i8 = "\\\\.\\pipe\\openssh-ssh-agent\0".as_ptr() as *const i8;

struct Pipe(HANDLE);

impl Read for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        unsafe {
            let mut read = 0;
            if ReadFile(
                self.0,
                buf.as_mut_ptr() as *mut c_void,
                buf.len() as u32,
                &mut read,
                null_mut(),
            ) == 0
            {
                Err(io::Error::last_os_error())
            } else {
                Ok(read as usize)
            }
        }
    }
}

impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        unsafe {
            let mut written = 0;
            if WriteFile(
                self.0,
                buf.as_ptr() as *const c_void,
                buf.len() as u32,
                &mut written,
                null_mut(),
            ) == 0
            {
                return Err(io::Error::last_os_error());
            };
            Ok(written as usize)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        unsafe {
            if FlushFileBuffers(self.0) == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }
}

impl ReadWrite for Pipe {
    fn read(&mut self) -> &mut dyn Read {
        self
    }

    fn write(&mut self) -> &mut dyn Write {
        self
    }
}

pub fn listen_named_pipe(agent: Arc<Mutex<Agent>>) -> Result<()> {
    unsafe {
        loop {
            let pipe = CreateNamedPipeA(
                PIPE_NAME,
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                1024,
                1024,
                0,
                null_mut(),
            );
            if pipe == INVALID_HANDLE_VALUE {
                check_error()?;
            }

            if ConnectNamedPipe(pipe, null_mut()) == 0 {
                check_error()?;
            }

            let requester = {
                let mut process_id: ULONG = 0;
                if GetNamedPipeClientProcessId(pipe, &mut process_id) == 0 {
                    None
                } else {
                    Some(process_id)
                }
            }.and_then(|pid| {
                let agent = agent.lock().unwrap();
                collect_requester_info(agent.config(), pid).ok()
            });

            let agent = agent.clone();
            let pipe = pipe as u64;
            std::thread::spawn(move || {
                let pipe = pipe as HANDLE;

                connection_handler(agent, &mut Pipe(pipe), RequestInfo {
                    channel: "Pipe",
                    requester,
                });

                DisconnectNamedPipe(pipe);
                CloseHandle(pipe);
            });
        }
    }
}
