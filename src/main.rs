use std::fs::{create_dir_all, File};

use clap::{App, Arg};
use log::error;
use log::LevelFilter;
use simplelog::{CombinedLogger, ConfigBuilder, SimpleLogger, WriteLogger};
use wrapperrs::{Result, ResultExt};

use copy_id::copy_id;

use crate::agent::Agent;
use crate::config::load_config;
use crate::platform::config_dir;

mod agent;
mod config;
mod copy_id;
mod key;
mod utils;

#[cfg(windows)]
#[path = "./platform/win/mod.rs"]
mod platform;
#[cfg(unix)]
#[path = "platform/unix/mod.rs"]
mod platform;

const NAME: &str = "ssh-keys";

fn main() {
    if let Err(err) = (|| {
        let logger_config = ConfigBuilder::new()
            .set_location_level(LevelFilter::Error)
            .build();
        create_dir_all(config_dir()).expect("create config dir");
        let log_file = File::create(config_dir().join("trace.log")).expect("create log file");
        CombinedLogger::init(vec![
            SimpleLogger::new(LevelFilter::Trace, logger_config.clone()),
            WriteLogger::new(LevelFilter::Trace, logger_config, log_file)
        ]).expect("init logger");

        let opts = App::new(NAME)
            .subcommand(
                App::new("copy-id")
                    .arg(Arg::with_name("username@host").required(true))
                    .arg(Arg::with_name("key").help("key name").required(true))
                    .arg(Arg::with_name("port").short("-p").default_value("22"))
                    .arg(Arg::with_name("erase").short("-e").help("Remove all keys")),
            )
            .get_matches();

        let config = load_config().wrap_err("load config")?;
        config.save()?;
        let agent = Agent::new(config);

        match opts.subcommand() {
            ("copy-id", opts) => copy_id(&agent, opts.unwrap()),
            _ => platform::serve(agent),
        }?;

        Ok(()) as Result<()>
    })() {
        error!("{}", err);
    };
}
