//! Single-instance mode (Windows), modeled on wispterm's design:
//! a session-local named mutex elects the first instance, which runs a
//! localhost TCP server and records {port, pid} in a discovery file under
//! the config dir. Later invocations read the file, forward their target
//! path (or their own cwd) and exit; the first instance opens it in a new
//! tab and brings its window to the foreground.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use xilem::core::MessageProxy;

const MUTEX_NAME: &str = "Local\\amux-SingleInstance";
const DISCOVERY_BASENAME: &str = "single-instance.json";
const MAX_PATH_BYTES: usize = 4096;

#[cfg(target_os = "windows")]
mod win {
    use super::MUTEX_NAME;
    use std::ffi::c_void;

    extern "system" {
        fn CreateMutexW(attrs: *mut c_void, initial_owner: i32, name: *const u16) -> *mut c_void;
        fn GetLastError() -> u32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn WaitForSingleObject(handle: *mut c_void, ms: u32) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn GetCurrentProcessId() -> u32;
    }

    const ERROR_ALREADY_EXISTS: u32 = 183;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_TIMEOUT: u32 = 0x102;

    /// Returns Some(handle) when this process is the first instance.
    pub fn acquire_mutex() -> Option<isize> {
        let name: Vec<u16> = MUTEX_NAME.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let h = CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr());
            if h.is_null() {
                return None;
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(h);
                return None;
            }
            // Keep the handle alive for the process lifetime.
            Some(h as isize)
        }
    }

    pub fn current_pid() -> u32 {
        unsafe { GetCurrentProcessId() }
    }

    pub fn is_process_alive(pid: u32) -> bool {
        unsafe {
            let h = OpenProcess(SYNCHRONIZE, 0, pid);
            if h.is_null() {
                return false;
            }
            let alive = WaitForSingleObject(h, 0) == WAIT_TIMEOUT;
            CloseHandle(h);
            alive
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod win {
    pub fn acquire_mutex() -> Option<isize> {
        None
    }
    pub fn current_pid() -> u32 {
        std::process::id()
    }
    pub fn is_process_alive(_pid: u32) -> bool {
        false
    }
}

/// Shared state between the accept thread and the UI thread.
pub struct ServerState {
    pending: Mutex<Vec<String>>,
    activate: AtomicBool,
    proxy: Mutex<Option<MessageProxy<()>>>,
    discovery_path: PathBuf,
}

impl ServerState {
    /// Hand the event-loop waker to the server thread.
    pub fn set_proxy(&self, proxy: MessageProxy<()>) {
        *self.proxy.lock().unwrap() = Some(proxy);
    }

    /// Drain queued requests: paths to open plus a window-activate flag.
    pub fn take_requests(&self) -> (Vec<String>, bool) {
        let paths = std::mem::take(&mut *self.pending.lock().unwrap());
        let activate = self.activate.swap(false, Ordering::AcqRel);
        (paths, activate)
    }

    pub fn shutdown(&self) {
        let _ = std::fs::remove_file(&self.discovery_path);
    }
}

/// Try to become the first instance and start the IPC server.
/// Returns `None` when another live instance holds the mutex, or on
/// non-Windows platforms where this mode is not implemented.
pub fn start_first_instance() -> Option<Arc<ServerState>> {
    if win::acquire_mutex().is_none() {
        return None;
    }

    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(_) => return None,
    };
    let port = match listener.local_addr() {
        Ok(a) => a.port(),
        Err(_) => return None,
    };
    let dir = config_dir();
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
    let discovery_path = dir.join(DISCOVERY_BASENAME);
    let body = format!("{{\"port\":{},\"pid\":{}}}", port, win::current_pid());
    if std::fs::write(&discovery_path, body).is_err() {
        return None;
    }

    let state = Arc::new(ServerState {
        pending: Mutex::new(Vec::new()),
        activate: AtomicBool::new(false),
        proxy: Mutex::new(None),
        discovery_path,
    });

    let server_state = Arc::downgrade(&state);
    thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(stream) = conn else { continue };
            let Some(state) = server_state.upgrade() else { return };
            handle_connection(&state, stream);
        }
    });

    Some(state)
}

fn handle_connection(state: &Arc<ServerState>, stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    // read_line returns Ok(0) on EOF.
    if reader.read_line(&mut line).is_err() {
        return;
    }
    line.truncate(line.trim_end_matches(['\r', '\n']).len());

    state.activate.store(true, Ordering::Release);
    if !line.is_empty() && line.len() <= MAX_PATH_BYTES {
        state.pending.lock().unwrap().push(line);
    }
    if let Some(proxy) = &*state.proxy.lock().unwrap() {
        let _ = proxy.message(());
    }
}

/// Second-instance client: forward `path` (or this process's cwd when None)
/// to the running first instance. Returns false if unreachable, in which
/// case the caller should start its own window.
pub fn forward_to_running(path: Option<&std::ffi::OsStr>) -> bool {
    let content = match std::fs::read_to_string(config_dir().join(DISCOVERY_BASENAME)) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let (Some(port), Some(pid)) = parse_discovery(&content) else {
        return false;
    };
    if pid != 0 && !win::is_process_alive(pid) {
        let _ = std::fs::remove_file(config_dir().join(DISCOVERY_BASENAME));
        return false;
    }

    let payload = match path {
        Some(p) => p.to_string_lossy().into_owned(),
        None => match std::env::current_dir() {
            Ok(d) => d.to_string_lossy().into_owned(),
            Err(_) => return false,
        },
    };

    let addr = match format!("127.0.0.1:{}", port).parse() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    let mut msg = payload;
    msg.push('\n');
    stream.write_all(msg.as_bytes()).is_ok() && stream.flush().is_ok()
}

fn parse_discovery(content: &str) -> (Option<u16>, Option<u32>) {
    let mut port = None;
    let mut pid = None;
    for part in content
        .trim_matches(['{', '}'])
        .split(',')
    {
        let mut kv = part.splitn(2, ':');
        let key = kv.next().unwrap_or("").trim_matches('"');
        let val = kv.next().unwrap_or("").trim().parse::<u64>().ok();
        match (key, val) {
            ("port", Some(v)) if (1..=u64::from(u16::MAX)).contains(&v) => port = Some(v as u16),
            ("pid", Some(v)) if v > 0 && v <= u64::from(u32::MAX) => pid = Some(v as u32),
            _ => {}
        }
    }
    (port, pid)
}

fn config_dir() -> PathBuf {
    crate::config::Config::path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::parse_discovery;

    #[test]
    fn discovery_roundtrip() {
        let (port, pid) = parse_discovery("{\"port\":54321,\"pid\":4242}");
        assert_eq!(port, Some(54321));
        assert_eq!(pid, Some(4242));
    }

    #[test]
    fn discovery_rejects_garbage() {
        let (port, pid) = parse_discovery("not json");
        assert_eq!(port, None);
        assert_eq!(pid, None);
        let (port, _) = parse_discovery("{\"port\":0,\"pid\":7}");
        assert_eq!(port, None);
    }
}
