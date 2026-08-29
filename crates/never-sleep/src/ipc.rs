#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::thread;

use crate::paths::{ensure_data_dir, ipc_socket_path};
use crate::protocol::{IpcRequest, IpcResponse};

pub enum IpcIncoming {
    Request {
        req: IpcRequest,
        reply: Sender<IpcResponse>,
    },
}

pub fn remove_stale_socket() {
    let path = ipc_socket_path();
    if path.exists() {
        // 若已有存活服务，探测成功就别删。
        if ping().is_ok() {
            return;
        }
        let _ = std::fs::remove_file(&path);
    }
}

pub fn ping() -> Result<IpcResponse, String> {
    send(&IpcRequest::Ping)
}

pub fn send(req: &IpcRequest) -> Result<IpcResponse, String> {
    let path = ipc_socket_path();
    let mut stream = UnixStream::connect(&path).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .ok();
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
pub fn spawn_server(tx: Sender<IpcIncoming>) -> Result<(), String> {
    ensure_data_dir().map_err(|e| e.to_string())?;
    remove_stale_socket();
    let path = ipc_socket_path();
    if path.exists() && ping().is_ok() {
        return Err("already_running".into());
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(|e| e.to_string())?;
    thread::Builder::new()
        .name("never-sleep-ipc".into())
        .spawn(move || {
            for conn in listener.incoming() {
                let Ok(stream) = conn else { continue };
                handle_conn(stream, &tx);
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn handle_conn(stream: UnixStream, tx: &Sender<IpcIncoming>) {
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
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap_or_else(|_| IpcResponse::err("超时"));
    let _ = write_resp(stream, &resp);
}

fn write_resp(mut stream: UnixStream, resp: &IpcResponse) -> std::io::Result<()> {
    let line = serde_json::to_string(resp)?;
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    Ok(())
}

pub fn socket_exists() -> bool {
    Path::new(&ipc_socket_path()).exists()
}
