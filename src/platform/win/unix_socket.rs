use std::fs::{create_dir_all, remove_file};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use uds_windows::{UnixListener, UnixStream};
use winapi::um::knownfolders::FOLDERID_Profile;
use wrapperrs::Result;

use crate::agent::{Agent, RequestInfo};
use crate::utils::{connection_handler, ReadWrite};

use super::utils::get_known_folder;

impl ReadWrite for UnixStream {
    fn read(&mut self) -> &mut dyn Read {
        self
    }

    fn write(&mut self) -> &mut dyn Write {
        self
    }
}

pub fn listen_unix_socket(agent: Arc<Mutex<Agent>>) -> Result<()> {
    let socket_path = get_known_folder(FOLDERID_Profile).join(".ssh/auth_sock");

    create_dir_all(socket_path.parent().unwrap())?;
    #[allow(unused_must_use)]
        {
            remove_file(&socket_path);
        }
    let listener = UnixListener::bind(socket_path)?;

    for stream in listener.incoming() {
        let mut stream = stream?;
        let agent = agent.clone();
        std::thread::spawn(move || {
            connection_handler(agent, &mut stream, RequestInfo {
                channel: "Unix",
                requester: None,
            });
        });
    }

    Ok(())
}
