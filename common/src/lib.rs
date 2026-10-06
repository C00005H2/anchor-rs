use std::path::PathBuf;
use once_cell::sync::Lazy;

pub const HOST: &str = "127.0.0.1";

pub const HTTP_PORT: u16 = 10800;
pub const HTTPS_PORT: u16 = 10443;

pub const GAMESERVER_LOGIN_PORT: u16 = 10403;

pub const GAMESERVER_PORT: u16 = 8702;

pub static CERT_FILE_PATH: Lazy<PathBuf> =
    Lazy::new(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../cert/localhost.crt"));

pub static KEY_FILE_PATH: Lazy<PathBuf> =
    Lazy::new(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../cert/localhost.key"));
pub const GAMESERVER: &str = "127.0.0.1";


pub static DATA_DIRECTORY: Lazy<PathBuf> = Lazy::new(|| {
    std::env::var("DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data"))
});


pub fn init_tracing() {
    #[cfg(target_os = "windows")]
    let _ = ansi_term::enable_ansi_support();

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("tcpserver=debug,common=debug,info"));

    // Tests, embedded callers, and multiple services may already have installed
    // a global subscriber; initialization should not panic in those cases.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .try_init();
}