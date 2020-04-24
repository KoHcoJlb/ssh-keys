use std::io::{Read, Write};
use std::io::ErrorKind::{BrokenPipe, UnexpectedEof};
use std::sync::{Arc, Mutex};

use byteorder::{BigEndian, ReadBytesExt};
use log::error;
use wrapperrs::{ErrorExt, Result};

use crate::agent::{Agent, RequestInfo};
use crate::agent::wire::WriteExt;

pub struct Finally<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> Finally<F> {
    pub fn new(f: F) -> Self {
        Self(Some(f))
    }
}

impl<F: FnOnce()> Drop for Finally<F> {
    fn drop(&mut self) {
        self.0.take().unwrap()();
    }
}

pub trait ReadWrite {
    fn read(&mut self) -> &mut dyn Read;

    fn write(&mut self) -> &mut dyn Write;
}

pub fn connection_handler<RW: ReadWrite>(agent: Arc<Mutex<Agent>>, rw: &mut RW, info: RequestInfo) {
    if let Err(err) = (|| -> Result<()> {
        loop {
            let len = match rw.read().read_u32::<BigEndian>() {
                Ok(0) => return Ok(()),
                Ok(len) => len,
                Err(err) if err.kind() == UnexpectedEof || err.kind() == BrokenPipe => {
                    return Ok(());
                }
                Err(err) => {
                    return Err(err.into());
                }
            };

            let mut buf = vec![0; len as usize];
            rw.read().read_exact(&mut buf)?;

            let resp = {
                let mut lock = agent.lock().unwrap();
                lock.handle_request(&buf, &info)
            };

            rw.write().write_string(resp)?;
        }
    })() {
        error!("{}", err.wrap("connection error"));
    };
}

#[macro_export]
macro_rules! log_error {
    (catch $res:expr, $arg0:tt) => ( log_error!(catch $res, $arg0,) );
    (catch $res:expr, $arg0:tt, $($args:tt)*) => (
        $res.map_err(|err| {
            error!($arg0, err, $($args)*);
            ()
        })?
    );
    ($res:expr, $($args:tt)*) => {
        $res.map_err(|err| {
            error!($($args)*);
            ()
        })
    }
}
