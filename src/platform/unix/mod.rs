use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::fs::remove_file;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use wrapperrs::{Error, ErrorExt, Result, ResultExt};

use crate::agent::{Agent, SSH_AGENT_FAILURE};

const SOCK_PATH: &str = "/tmp/auth_sock";

fn connection_handler(agent: Arc<Mutex<Box<Agent>>>, mut stream: UnixStream) -> Result<()> {
    loop {
        let len = stream.read_u32::<BigEndian>()?;

        let mut content = vec![0; len as usize];
        stream.read_exact(&mut content).wrap_err("read content")?;

        let mut agent = agent.lock().unwrap();
        let resp = match agent.handle_request(&content) {
            Ok(resp) => resp,
            Err(err) => {
                eprintln!("{}", err.wrap("error handling message"));
                vec![SSH_AGENT_FAILURE]
            }
        };
        stream.write_u32::<BigEndian>(resp.len() as u32)?;
        stream.write_all(&resp)?;
    }
}

pub fn serve(agent: Box<Agent>) -> Result<()> {
    let agent_mtx = Arc::new(Mutex::new(agent));

    remove_file(SOCK_PATH);
    let sock_server = UnixListener::bind(SOCK_PATH).wrap_err("bind")?;
    for stream in sock_server.incoming() {
        let agent_mtx = agent_mtx.clone();
        std::thread::spawn(move || {
            connection_handler(agent_mtx.clone(), stream.unwrap());
        });
    }
    Ok(())
}
