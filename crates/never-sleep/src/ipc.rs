#[cfg(any(test, target_os = "macos"))]
use std::io::{self, ErrorKind};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use crate::paths::ipc_socket_path;
use crate::protocol::{IpcRequest, IpcResponse};

#[cfg(target_os = "macos")]
use std::os::unix::net::UnixListener;
#[cfg(target_os = "macos")]
use std::sync::mpsc::Sender;
#[cfg(target_os = "macos")]
use std::thread;

#[cfg(target_os = "macos")]
use crate::paths::ensure_data_dir;

#[cfg(target_os = "macos")]
pub enum IpcIncoming {
    Request {
        req: IpcRequest,
        reply: Sender<IpcResponse>,
    },
}

#[cfg(target_os = "macos")]
fn connect_live() -> io::Result<UnixStream> {
    UnixStream::connect(ipc_socket_path())
}

#[cfg(any(test, target_os = "macos"))]
fn is_absent(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        ErrorKind::ConnectionRefused | ErrorKind::NotFound | ErrorKind::ConnectionReset
    )
}

pub fn send(req: &IpcRequest) -> Result<IpcResponse, String> {
    let path = ipc_socket_path();
    let mut stream = UnixStream::connect(&path).map_err(|e| e.to_string())?;
    let timeout = Duration::from_secs(3);
    stream.set_read_timeout(Some(timeout)).ok();
    stream.set_write_timeout(Some(timeout)).ok();
    let line = serde_json::to_string(req).map_err(|e| e.to_string())?;
    stream
        .write_all(line.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);
    let mut resp = String::new();
    reader.read_line(&mut resp).map_err(|e| e.to_string())?;
    serde_json::from_str(&resp).map_err(|e| e.to_string())
}

pub fn try_send(req: &IpcRequest) -> Option<IpcResponse> {
    send(req).ok()
}

/// 在后台线程接受连接，把请求发到主循环。
#[cfg(target_os = "macos")]
pub fn spawn_server(tx: Sender<IpcIncoming>) -> Result<(), String> {
    ensure_data_dir().map_err(|e| e.to_string())?;
    let path = ipc_socket_path();
    if path.exists() {
        match connect_live() {
            Ok(_) => return Err("already_running".into()),
            Err(e) if is_absent(&e) => {
                let _ = std::fs::remove_file(&path);
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let listener = UnixListener::bind(&path).map_err(|e| e.to_string())?;
    thread::Builder::new()
        .name("never-sleep-ipc".into())
        .spawn(move || {
            for conn in listener.incoming() {
                let Ok(stream) = conn else { continue };
                let tx = tx.clone();
                thread::spawn(move || handle_conn(stream, &tx));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn handle_conn(stream: UnixStream, tx: &Sender<IpcIncoming>) {
    let timeout = Duration::from_secs(3);
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let req: IpcRequest = match serde_json::from_str(line.trim()) {
        Ok(r) => r,
        Err(e) => {
            let _ = write_resp(stream, &IpcResponse::err(e.to_string()));
            return;
        }
    };
    let (rtx, rrx) = std::sync::mpsc::channel();
    if tx.send(IpcIncoming::Request { req, reply: rtx }).is_err() {
        return;
    }
    let resp = rrx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or_else(|_| IpcResponse::err(crate::persist::load_config().tr().ipc_timeout()));
    let _ = write_resp(stream, &resp);
}

#[cfg(target_os = "macos")]
fn write_resp(mut stream: UnixStream, resp: &IpcResponse) -> std::io::Result<()> {
    let line = serde_json::to_string(resp)?;
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::IpcRequest;

    #[test]
    fn try_send_without_listener_is_none() {
        let _isolated = crate::paths::TestDataDir::install();
        assert!(try_send(&IpcRequest::Status).is_none());
        assert!(try_send(&IpcRequest::Ping).is_none());
    }

    #[test]
    fn absent_errors_include_not_found() {
        let err = io::Error::from(ErrorKind::NotFound);
        assert!(is_absent(&err));
        let err = io::Error::from(ErrorKind::PermissionDenied);
        assert!(!is_absent(&err));
    }
}
