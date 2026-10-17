//! Keys in a box (read-only): live facts about the Firecracker micro-VMs that hold the keys (ADR-028).
//!
//! - `vm-status`: for the organizer, crew or treasury box (or all three) reports what Firecracker itself
//!   says (state, version, vCPUs, memory, network interfaces, read-only disks), how long the VM has run,
//!   which pinholes its vsock bridge opens, whether the wallet API answers through it, and whether the
//!   VM owns that wallet's data. Output in `msg.payload` and `msg.vm.<box>`.
//!
//! Nothing here can start, stop or change a VM, and there is no node that can: the AI can see the
//! boxes, not open them.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::runtime::flow::Flow;
use crate::runtime::model::json::RedFlowNodeConfig;
use crate::runtime::model::{MsgHandle, Variant};
use crate::runtime::nodes::*;
use n2link_macro::*;

use super::{set_payload, status};

/// The boxes: (name shown to people, role used by `scripts/vm/boot.sh`, wallet API port on the host).
pub const BOXES: [(&str, &str, u16); 3] =
    [("organizer", "payer", 8711), ("crew", "recipient", 8712), ("treasury", "wdk", 8713)];

fn repo_root() -> PathBuf {
    std::env::var_os("POWEREDGE_ROOT").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn run_dir() -> PathBuf {
    std::env::var_os("POWEREDGE_VM_RUN").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp/scratchmypay-vm"))
}

/// Minimal HTTP/1.1 GET over any stream; reads exactly Content-Length (Firecracker keeps connections alive).
async fn http_get<S: AsyncRead + AsyncWrite + Unpin>(mut stream: S, uri: &str) -> Option<(u16, String)> {
    let request =
        format!("GET {uri} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.ok()?;
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = find(&buf, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).to_string();
            let length = head
                .lines()
                .find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("content-length")))
                .and_then(|(_, v)| v.trim().parse::<usize>().ok());
            if let Some(length) = length
                && buf.len() >= end + 4 + length
            {
                break;
            }
        }
        if buf.len() > 256 * 1024 {
            return None;
        }
    }
    let end = find(&buf, b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&buf[..end]);
    let code = head.split_whitespace().nth(1)?.parse().ok()?;
    Some((code, String::from_utf8_lossy(&buf[end + 4..]).to_string()))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

async fn firecracker_get(sock: &Path, uri: &str) -> Option<Value> {
    let call = async {
        let stream = tokio::net::UnixStream::connect(sock).await.ok()?;
        let (code, body) = http_get(stream, uri).await?;
        if code != 200 {
            return None;
        }
        serde_json::from_str(body.trim()).ok()
    };
    tokio::time::timeout(Duration::from_secs(2), call).await.ok().flatten()
}

async fn wallet_api_ok(port: u16) -> bool {
    let call = async {
        let stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.ok()?;
        let (code, body) = http_get(stream, "/health").await?;
        Some(code == 200 && body.contains("ok"))
    };
    tokio::time::timeout(Duration::from_secs(3), call).await.ok().flatten().unwrap_or(false)
}

fn read_pid(name: &str) -> Option<u32> {
    let path = repo_root().join("artifacts").join("run").join("pids").join(format!("{name}.pid"));
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn cmdline(pid: u32) -> Option<Vec<String>> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    Some(raw.split(|b| *b == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).into_owned()).collect())
}

/// Seconds since the process started (/proc/<pid>/stat field 22, clock ticks at the usual 100 Hz).
fn uptime_secs(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let start_ticks: f64 = stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse().ok()?;
    let up: f64 = std::fs::read_to_string("/proc/uptime").ok()?.split_whitespace().next()?.parse().ok()?;
    Some((up - start_ticks / 100.0).max(0.0) as u64)
}

/// Where the box's bridge lets traffic go: one entry per pinhole, plus the organizer -> crew delivery.
fn openings(role: &str) -> Vec<String> {
    let Some(args) = read_pid(&format!("vm-{role}-bridge")).and_then(cmdline) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for pair in args.windows(2) {
        let target = pair[1].split_once('=').map(|(_, t)| t.to_owned()).unwrap_or_default();
        match pair[0].as_str() {
            "--pinhole" if target.starts_with("127.0.0.1:") => out.push(format!("crew on the host ({target})")),
            "--pinhole" => out.push(target),
            "--relay" => out.push("crew VM (vsock relay)".to_owned()),
            _ => {}
        }
    }
    out
}

fn human_uptime(secs: u64) -> String {
    match secs {
        s if s < 90 => format!("{s}s"),
        s if s < 5400 => format!("{}m", s / 60),
        s => format!("{}h{:02}m", s / 3600, (s % 3600) / 60),
    }
}

/// Live status of one box. Never changes anything.
pub async fn box_status(name: &str, role: &str, port: u16) -> Value {
    let owned_by_vm = repo_root().join("datadirs").join(format!("{role}.IN_VM")).exists();
    let pid = read_pid(&format!("vm-{role}"))
        .filter(|p| cmdline(*p).is_some_and(|c| c.first().is_some_and(|a| a.ends_with("firecracker"))));
    let sock = run_dir().join(role).join("api.sock");
    let (instance, config) = match pid {
        Some(_) => (firecracker_get(&sock, "/").await, firecracker_get(&sock, "/vm/config").await),
        None => (None, None),
    };
    let nics = config.as_ref().and_then(|c| c["network-interfaces"].as_array()).map(Vec::len);
    let drives: Vec<Value> = config
        .as_ref()
        .and_then(|c| c["drives"].as_array())
        .map(|d| {
            d.iter()
                .map(|x| json!({"id": x["drive_id"], "readOnly": x["is_read_only"], "root": x["is_root_device"]}))
                .collect()
        })
        .unwrap_or_default();
    let running = instance.as_ref().is_some_and(|i| i["state"] == "Running");
    let uptime = pid.and_then(uptime_secs);
    let api = wallet_api_ok(port).await;
    let summary = if running {
        let cards = match nics {
            Some(0) | None => "no network card".to_owned(),
            Some(1) => "1 network card".to_owned(),
            Some(n) => format!("{n} network cards"),
        };
        format!(
            "running · {cards} · {} opening{} · up {}",
            openings(role).len(),
            if openings(role).len() == 1 { "" } else { "s" },
            uptime.map(human_uptime).unwrap_or_else(|| "?".into())
        )
    } else if owned_by_vm {
        "VM down (it owns the wallet data)".to_owned()
    } else {
        "not in a VM (host process)".to_owned()
    };
    json!({
        "box": name,
        "role": role,
        "inVm": owned_by_vm,
        "running": running,
        "pid": pid,
        "uptimeSeconds": uptime,
        "firecracker": instance.as_ref().map(|i| json!({"state": i["state"], "version": i["vmm_version"], "id": i["id"]})),
        "vcpus": config.as_ref().map(|c| c["machine-config"]["vcpu_count"].clone()),
        "memoryMib": config.as_ref().map(|c| c["machine-config"]["mem_size_mib"].clone()),
        "networkInterfaces": nics,
        "drives": drives,
        "vsockCid": config.as_ref().map(|c| c["vsock"]["guest_cid"].clone()),
        "openings": openings(role),
        "walletApi": if api { "ok" } else { "down" },
        "summary": summary,
    })
}

/// All boxes, in a fixed order (organizer, crew, treasury).
pub async fn all_boxes() -> Vec<Value> {
    let mut out = Vec::with_capacity(BOXES.len());
    for (name, role, port) in BOXES {
        out.push(box_status(name, role, port).await);
    }
    out
}

// ---------------------------------------------------------------- vm-status
crate::node_hints!("vm-status", caps = ["host"], input = "any", outs = ["status" => "object"]);

#[derive(Debug, Deserialize)]
struct VmStatusConfig {
    #[serde(default, rename = "box")]
    which: String,
}

#[flow_node("vm-status", red_name = "vm-status", inputs = 1, outputs = 1)]
struct VmStatusNode {
    base: BaseFlowNodeState,
    which: String,
}

impl VmStatusNode {
    fn build(
        _f: &Flow,
        base: BaseFlowNodeState,
        config: &RedFlowNodeConfig,
        _o: Option<&config::Config>,
    ) -> crate::Result<Box<dyn FlowNodeBehavior>> {
        let raw = VmStatusConfig::deserialize(&config.rest)?;
        let which = if raw.which.is_empty() { "all".to_owned() } else { raw.which };
        if which != "all" && !BOXES.iter().any(|(n, _, _)| *n == which) {
            return Err(crate::N2linkError::invalid_operation(&format!(
                "vm-status: box must be organizer, crew, treasury or all (got '{which}')"
            )));
        }
        Ok(Box::new(VmStatusNode { base, which }))
    }

    async fn handle(&self, msg: MsgHandle, cancel: CancellationToken) -> crate::Result<()> {
        let boxes: Vec<Value> = if self.which == "all" {
            all_boxes().await
        } else {
            let (name, role, port) = BOXES.iter().find(|(n, _, _)| *n == self.which).copied().unwrap_or(BOXES[0]);
            vec![box_status(name, role, port).await]
        };
        let up = boxes.iter().filter(|b| b["running"] == true).count();
        let (fill, shape, text) = if self.which == "all" {
            let fill = if up == boxes.len() {
                StatusFill::Green
            } else if up == 0 {
                StatusFill::Red
            } else {
                StatusFill::Yellow
            };
            (fill, StatusShape::Dot, format!("{up}/{} boxes running · keys in VMs", boxes.len()))
        } else {
            let b = &boxes[0];
            let fill = if b["running"] == true {
                StatusFill::Green
            } else if b["inVm"] == true {
                StatusFill::Red
            } else {
                StatusFill::Yellow
            };
            (
                fill,
                if b["running"] == true { StatusShape::Dot } else { StatusShape::Ring },
                b["summary"].as_str().unwrap_or("").to_owned(),
            )
        };
        {
            let mut guard = msg.write().await;
            let mut vm = guard
                .get("vm")
                .and_then(|v| serde_json::to_value(v).ok())
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({}));
            for b in &boxes {
                if let Some(name) = b["box"].as_str() {
                    vm[name] = b.clone();
                }
            }
            guard.set("vm".to_owned(), Variant::from(vm));
        }
        let payload = if self.which == "all" {
            json!({"ok": up == boxes.len(), "running": up, "total": boxes.len(), "boxes": boxes})
        } else {
            boxes[0].clone()
        };
        set_payload(&msg, payload, None).await;
        self.report_status(status(fill, shape, text), cancel.clone()).await;
        self.fan_out_one(Envelope { port: 0, msg }, cancel).await
    }
}

#[async_trait]
impl FlowNodeBehavior for VmStatusNode {
    fn get_base(&self) -> &BaseFlowNodeState {
        &self.base
    }

    async fn run(self: Arc<Self>, stop_token: CancellationToken) {
        while !stop_token.is_cancelled() {
            let cancel = stop_token.child_token();
            with_uow(self.as_ref(), cancel.child_token(), |node, msg| async move {
                // Read-only node: on Deploy/stop, drop the in-flight call instead of holding
                // the whole redeploy until it times out (wallet calls may take minutes).
                let stop = cancel.clone();
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => Ok(()),
                    result = node.handle(msg, cancel) => result,
                }
            })
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn http_get_reads_exactly_content_length_on_a_kept_alive_connection() {
        let (client, mut server) = tokio::io::duplex(4096);
        let serve = tokio::spawn(async move {
            let mut req = [0u8; 512];
            let _ = server.read(&mut req).await;
            server
                .write_all(
                    b"HTTP/1.1 200 \r\nConnection: keep-alive\r\nContent-Length: 19\r\n\r\n{\"state\":\"Running\"}",
                )
                .await
                .unwrap();
            // keep the connection open: a reader that waits for EOF would hang here
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        let got = tokio::time::timeout(Duration::from_secs(1), http_get(client, "/")).await.expect("no hang");
        let (code, body) = got.expect("response");
        assert_eq!(code, 200);
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["state"], "Running");
        serve.abort();
    }

    #[test]
    fn human_uptime_is_short() {
        assert_eq!(human_uptime(42), "42s");
        assert_eq!(human_uptime(600), "10m");
        assert_eq!(human_uptime(7380), "2h03m");
    }

    #[tokio::test]
    async fn a_box_with_no_vm_and_no_marker_reads_as_host_process() {
        // SAFETY: test-only env change, single-threaded test body
        unsafe {
            std::env::set_var("POWEREDGE_ROOT", "/nonexistent-poweredge-root");
            std::env::set_var("POWEREDGE_VM_RUN", "/nonexistent-poweredge-run");
        }
        let b = box_status("crew", "recipient", 1).await;
        assert_eq!(b["running"], false);
        assert_eq!(b["inVm"], false);
        assert_eq!(b["summary"], "not in a VM (host process)");
    }
}
