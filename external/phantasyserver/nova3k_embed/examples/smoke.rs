//! Nova3K: runs the embedded server on a PC for a quick test (CI): `cargo run --example smoke -- <dir> <seconds>`
//! Server log lines are printed to stdout, like they go to vita3k.log in the emulator.
use std::ffi::{CStr, CString, c_char};

extern "C" fn print_log(level: i32, message: *const c_char) {
    let text = unsafe { CStr::from_ptr(message) }.to_string_lossy();
    println!("[{level}] {text}");
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().unwrap_or_else(|| "smoke_server".into());
    let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(30);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = std::fs::canonicalize(dir).unwrap();
    let c = CString::new(dir.to_str().unwrap()).unwrap();
    let r = unsafe { nova3k_embed::nova3k_pso2_server_start(c.as_ptr(), print_log) };
    println!("start -> {r}");
    std::thread::sleep(std::time::Duration::from_secs(seconds));
    println!("smoke: done");
}
