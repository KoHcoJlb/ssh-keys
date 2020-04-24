use std::io::{Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::path::Path;

use byteorder::WriteBytesExt;
use clap::ArgMatches;
use data_encoding::BASE64;
use ssh2::{OpenFlags, OpenType, Session};
use wrapperrs::{Error, Result, ResultExt};

use crate::agent::Agent;

pub fn copy_id(agent: &Agent, opts: &ArgMatches) -> Result<()> {
    let key_name = opts.value_of("key").unwrap();
    let key = agent
        .config()
        .keys
        .iter()
        .find(|k| k.name() == key_name)
        .ok_or(Error::new("key not found"))?;

    let (user, host) = {
        let split: Vec<&str> = opts.value_of("host").unwrap().split("@").collect();
        if let &[user, host] = split.as_slice() {
            (user, host)
        } else {
            return Err(Error::new("invalid host").into());
        }
    };

    let mut sess = Session::new()?;
    let port = opts
        .value_of("port")
        .unwrap()
        .parse::<u16>()
        .wrap_err("invalid port")?;
    sess.set_tcp_stream(TcpStream::connect((host, port)).wrap_err("connect")?);
    sess.handshake().wrap_err("handshake")?;

    let password = rpassword::read_password_from_tty(Some("Password: "))?;
    sess.userauth_password(user, &password)?;

    let mut channel = sess.channel_session()?;
    channel.exec("cd ; pwd")?;
    let mut s = String::new();
    channel.read_to_string(&mut s)?;

    let home = Path::new(s.trim_end());

    let sftp = sess.sftp()?;
    let ssh_dir = home.join(".ssh");
    if let Err(_) = sftp.stat(&ssh_dir) {
        sftp.mkdir(&ssh_dir, 0o700)?;
    }

    let authorized_keys = ssh_dir.join("authorized_keys");
    let mut file = sftp.open_mode(
        &authorized_keys,
        OpenFlags::CREATE | OpenFlags::READ | OpenFlags::WRITE,
        0o600,
        OpenType::File,
    )?;

    let mut str = String::new();
    if !opts.is_present("erase") {
        file.read_to_string(&mut str)?;
        file.seek(SeekFrom::End(0))?;
    }

    let public_key = key.public();
    let public_key_b64 = BASE64.encode(&public_key.encode());

    if !str.contains(&public_key_b64) {
        if !str.is_empty() && !str.ends_with("\n") {
            file.write_u8('\n' as u8)?;
        }

        file.write_all(
            format!(
                "{} {} {}\n",
                public_key.key_type(),
                public_key_b64,
                key.name()
            )
                .as_bytes(),
        )?;
        println!("Key successfully added");
    } else {
        println!("Key exists");
    };

    Ok(())
}
