pub use platform::*;

#[cfg(windows)]
mod win;
#[cfg(unix)]
mod unix;
