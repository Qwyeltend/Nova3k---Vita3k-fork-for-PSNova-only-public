//! Nova3K: PhantasyServer inside the emulator.
//!
//! PhantasyServer (https://github.com/PhantasyServer/PhantasyServer) is a PSO2 server by Anton Maltsev,
//! MIT license. Nova3K (by Qwyeltend) runs it in-process so PSO2 for PS Vita works offline. Without
//! his work this would not have been possible - thank you!
//!
//! C entry point: `nova3k_pso2_server_start(dir, log)`. `dir` is a folder in the emulator's data
//! (config files, databases with accounts and characters, keys). `log` receives the server's log lines.

use std::ffi::{CStr, CString, c_char};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static COM_DATA: &[u8] = include_bytes!(env!("NOVA3K_COM_DATA"));
static KEYPAIR: &str = include_str!("../keypair.pem");

pub type LogFn = extern "C" fn(level: i32, message: *const c_char);

struct CLogger {
    log: LogFn,
}

impl log::Log for CLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info || metadata.target().starts_with("pso2ship_server") && metadata.level() <= log::Level::Debug
    }
    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = match record.level() {
            log::Level::Error => 0,
            log::Level::Warn => 1,
            log::Level::Info => 2,
            log::Level::Debug => 3,
            log::Level::Trace => 4,
        };
        let text = format!("[{}] {}", record.target(), record.args());
        if let Ok(c) = CString::new(text.replace('\0', " ")) {
            (self.log)(level, c.as_ptr());
        }
    }
    fn flush(&self) {}
}

static LOGGER: OnceLock<CLogger> = OnceLock::new();
static STARTED: AtomicBool = AtomicBool::new(false);

const MASTER_SHIP_TOML: &str = r#"# Nova3K: written on every start
db_name = "master_ship.db"
registration_enabled = true
log_dir = "logs"
file_log_level = "INFO"
console_log_level = "OFF"
data_path = "com_data.mp"
"#;

const SHIP_TOML: &str = r#"# Nova3K: written on every start
server_name = "Nova3K"
db_name = "ship.db"
balance_port = 12000
min_ship_id = 1
max_ship_id = 10
key_file = "keypair.pem"
master_ship = "127.0.0.1:15000"
master_ship_psk = "nova3k_local_ship"
data_file = "com_data.mp"
log_dir = "logs"
file_log_level = "INFO"
console_log_level = "OFF"

[[blocks]]
name = "Nova3K Block 1"
max_players = 32
lobby_map = "lobby"
"#;

fn write_if_changed(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if std::fs::read(path).map(|old| old == data).unwrap_or(false) {
        return Ok(());
    }
    std::fs::write(path, data)
}

fn prepare(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir.join("logs"))?;
    write_if_changed(&dir.join("master_ship.toml"), MASTER_SHIP_TOML.as_bytes())?;
    write_if_changed(&dir.join("ship.toml"), SHIP_TOML.as_bytes())?;
    write_if_changed(&dir.join("keypair.pem"), KEYPAIR.as_bytes())?;
    write_if_changed(&dir.join("com_data.mp"), COM_DATA)?;
    Ok(())
}

/// Starts the server once. Returns 0 on start, 1 if it was already started, negative on error.
///
/// # Safety
/// `dir` must be a valid NUL-terminated UTF-8 path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nova3k_pso2_server_start(dir: *const c_char, log: LogFn) -> i32 {
    if STARTED.swap(true, Ordering::SeqCst) {
        return 1;
    }
    if LOGGER.set(CLogger { log }).is_ok() {
        let _ = log::set_logger(LOGGER.get().unwrap());
        log::set_max_level(log::LevelFilter::Debug);
    }
    // a panic inside the server would otherwise only print to stderr, which Android drops
    std::panic::set_hook(Box::new(|info| {
        log::error!("Nova3K: server panic: {info}");
    }));
    let Ok(dir) = (unsafe { CStr::from_ptr(dir) }).to_str() else {
        return -1;
    };
    let dir = PathBuf::from(dir);
    if let Err(e) = prepare(&dir) {
        log::error!("Nova3K: cannot prepare server folder {}: {e}", dir.display());
        return -2;
    }
    // PhantasyServer reads and writes its files relative to the working directory
    if let Err(e) = std::env::set_current_dir(&dir) {
        log::error!("Nova3K: cannot enter server folder {}: {e}", dir.display());
        return -3;
    }
    log::info!("Nova3K: starting PhantasyServer by Anton Maltsev (MIT license) in {}", dir.display());
    let spawned = std::thread::Builder::new().name("pso2-server".into()).spawn(|| {
        let runtime = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
            Ok(r) => r,
            Err(e) => {
                log::error!("Nova3K: cannot start the server runtime: {e}");
                return;
            }
        };
        runtime.block_on(async {
            tokio::spawn(async {
                if let Err(e) = master_ship::run().await {
                    log::error!("Nova3K: master ship stopped: {e}");
                }
            });
            // the ship registers itself at the master ship; retry until the master ship is up
            // (run in its own task, so a panic is caught and the ship is started again)
            for attempt in 1..=10 {
                tokio::time::sleep(Duration::from_secs(1)).await;
                match tokio::spawn(pso2ship_server::run()).await {
                    Ok(Ok(())) => break,
                    Ok(Err(e)) => log::warn!("Nova3K: ship server stopped (attempt {attempt}): {e}"),
                    Err(e) => log::error!("Nova3K: ship server crashed (attempt {attempt}): {e}"),
                }
            }
        });
    });
    match spawned {
        Ok(_) => 0,
        Err(_) => -4,
    }
}
