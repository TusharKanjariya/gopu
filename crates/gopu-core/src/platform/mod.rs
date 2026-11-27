//! OS access: the Gopu key, the focused field, overlay window styles.
//! One module per OS with the same free functions, picked at compile time.
//! macOS arrives with spike ticket 05.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;
