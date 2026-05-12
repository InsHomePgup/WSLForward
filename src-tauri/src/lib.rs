use std::collections::HashSet;
use std::net::ToSocketAddrs;
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::Duration;
use serde::{Deserialize, Serialize};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

// ── Data types ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PortProxyRule {
    pub listen_addr: String,
    pub listen_port: String,
    pub connect_addr: String,
    pub connect_port: String,
    pub win_open: bool,
    pub wsl_running: bool,
    pub docker_matches: Vec<DockerMatch>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DockerMatch {
    pub name: String,
    pub host_port: u16,
    pub container_port: Option<u16>,
    pub proto: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DockerContainer {
    pub id: String,
    pub name: String,
    pub ports: Vec<DockerPort>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DockerPort {
    pub host_ip: String,
    pub host_port: u16,
    pub container_port: Option<u16>,
    pub proto: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AllData {
    pub rules: Vec<PortProxyRule>,
    pub wsl_ports: Vec<u16>,
    pub docker_containers: Vec<DockerContainer>,
    pub errors: Vec<String>,
    pub debug_log: Vec<String>,
}

// ── Subprocess helper ────────────────────────────────────────────────────────

fn exec(args: &[&str]) -> Option<String> {
    exec_debug(args).0
}

fn exec_debug(args: &[&str]) -> (Option<String>, String) {
    if args.is_empty() {
        return (None, "empty command".into());
    }
    let mut cmd = Command::new(args[0]);
    cmd.args(&args[1..]);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            if out.status.success() {
                (Some(stdout.clone()), format!("ok ({} bytes)", stdout.len()))
            } else {
                let detail = if stderr.is_empty() { format!("exit {}", out.status) } else { stderr.trim().to_string() };
                (None, detail)
            }
        }
        Err(e) => (None, e.to_string()),
    }
}

fn exec_checked(args: &[&str]) -> Result<(), String> {
    if args.is_empty() {
        return Err("Empty command".into());
    }
    let mut cmd = Command::new(args[0]);
    cmd.args(&args[1..]);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

// ── Network helpers ──────────────────────────────────────────────────────────

fn port_open(host: &str, port: u16) -> bool {
    let addr_str = format!("{}:{}", host, port);
    addr_str
        .to_socket_addrs()
        .ok()
        .and_then(|mut it| it.next())
        .map(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok())
        .unwrap_or(false)
}

// ── Parsers ──────────────────────────────────────────────────────────────────

fn parse_netsh(raw: &str) -> Vec<(String, String, String, String)> {
    let lines: Vec<&str> = raw.lines().collect();
    let mut start = 0;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with("---") || t.to_lowercase().starts_with("address") {
            start = i + 1;
            break;
        }
    }
    lines[start..]
        .iter()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                Some((
                    parts[0].to_string(),
                    parts[1].to_string(),
                    parts[2].to_string(),
                    parts[3].to_string(),
                ))
            } else {
                None
            }
        })
        .collect()
}

fn parse_wsl_ports(output: &str) -> HashSet<u16> {
    let mut ports = HashSet::new();
    for line in output.lines() {
        if !line.to_uppercase().contains("LISTEN") {
            continue;
        }
        for part in line.split_whitespace() {
            let port_str = if part.starts_with('[') {
                // IPv6: [::]:8080
                part.split(']').nth(1).unwrap_or("").trim_start_matches(':')
            } else if let Some(pos) = part.rfind(':') {
                &part[pos + 1..]
            } else {
                continue;
            };
            let digits: String = port_str.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(p) = digits.parse::<u16>() {
                if p > 0 {
                    ports.insert(p);
                }
            }
        }
    }
    ports
}

fn parse_docker_output(raw: &str) -> Vec<DockerContainer> {
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            if parts.len() < 2 {
                return None;
            }
            let id = parts[0].to_string();
            let name = parts[1].to_string();
            let ports_str = if parts.len() >= 3 { parts[2] } else { "" };

            let ports = ports_str
                .split(',')
                .filter_map(|p| {
                    let p = p.trim();
                    if p.is_empty() {
                        return None;
                    }
                    // Format: 0.0.0.0:8080->80/tcp
                    let (host_part, rest) = p.split_once("->")?;
                    let (container_part, proto) = rest
                        .split_once('/')
                        .map(|(c, pr)| (c, pr.to_string()))
                        .unwrap_or((rest, "tcp".to_string()));

                    let (host_ip, host_port_str) = host_part
                        .rfind(':')
                        .map(|pos| (&host_part[..pos], &host_part[pos + 1..]))
                        .unwrap_or(("", host_part));

                    let host_port = host_port_str.parse::<u16>().ok()?;
                    Some(DockerPort {
                        host_ip: host_ip.to_string(),
                        host_port,
                        container_port: container_part.parse().ok(),
                        proto,
                    })
                })
                .collect();

            Some(DockerContainer { id, name, ports })
        })
        .collect()
}

// ── Data gathering ───────────────────────────────────────────────────────────

fn gather_wsl_ports() -> (HashSet<u16>, Vec<String>) {
    let fallbacks: &[&[&str]] = &[
        &["wsl", "ss", "-tulpn"],
        &["wsl", "sudo", "ss", "-tulpn"],
        &["wsl", "netstat", "-tulpn"],
        &["wsl", "sudo", "netstat", "-tulpn"],
    ];
    let mut log = Vec::new();
    for cmd in fallbacks {
        let label = cmd.join(" ");
        let (out, detail) = exec_debug(cmd);
        if let Some(output) = out {
            let ports = parse_wsl_ports(&output);
            log.push(format!("[WSL ports] `{}` → {} ports found\nraw:\n{}", label, ports.len(), output.trim()));
            return (ports, log);
        } else {
            log.push(format!("[WSL ports] `{}` → failed: {}", label, detail));
        }
    }
    (HashSet::new(), log)
}

fn gather_docker_containers() -> Vec<DockerContainer> {
    let fmt = "{{.ID}} {{.Names}} {{.Ports}}";
    let fallbacks: &[&[&str]] = &[
        &["docker", "ps", "--format", fmt],
        &["wsl", "docker", "ps", "--format", fmt],
        &["wsl", "-u", "root", "docker", "ps", "--format", fmt],
    ];
    for cmd in fallbacks {
        if let Some(out) = exec(cmd) {
            let containers = parse_docker_output(&out);
            if !containers.is_empty() {
                return containers;
            }
        }
    }
    Vec::new()
}

// ── Tauri commands ───────────────────────────────────────────────────────────

#[tauri::command]
fn is_admin() -> bool {
    Command::new("whoami")
        .args(["/groups"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map(|o| {
            // S-1-16-12288 = High Mandatory Level (elevated)
            String::from_utf8_lossy(&o.stdout).contains("S-1-16-12288")
        })
        .unwrap_or(false)
}

#[tauri::command]
async fn get_wsl_ip() -> Result<String, String> {
    exec(&[
        "wsl",
        "bash",
        "-c",
        r"ip addr show eth0 | grep -oP '(?<=inet\s)\d+(?:\.\d+){3}'",
    ])
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .ok_or_else(|| "Could not detect WSL IP from eth0".to_string())
}

#[tauri::command]
async fn get_all_data() -> AllData {
    let mut errors = Vec::new();

    let netsh_raw = exec(&["netsh", "interface", "portproxy", "show", "all"])
        .unwrap_or_else(|| {
            errors.push("netsh failed — ensure the app runs as Administrator".to_string());
            String::new()
        });

    let (wsl_ports, wsl_log) = gather_wsl_ports();
    let docker_containers = gather_docker_containers();
    let raw_rules = parse_netsh(&netsh_raw);

    let rules = raw_rules
        .into_iter()
        .map(|(la, lp, ca, cp)| {
            let check_host = if la == "0.0.0.0" || la == "*" {
                "127.0.0.1"
            } else {
                &la
            };
            let win_open = lp.parse::<u16>().map(|p| port_open(check_host, p)).unwrap_or(false);
            let cp_u16: u16 = cp.parse().unwrap_or(0);
            let wsl_running = wsl_ports.contains(&cp_u16);

            let docker_matches = docker_containers
                .iter()
                .flat_map(|c| {
                    c.ports.iter().filter_map(|p| {
                        if p.host_port == cp_u16 || p.container_port == Some(cp_u16) {
                            Some(DockerMatch {
                                name: c.name.clone(),
                                host_port: p.host_port,
                                container_port: p.container_port,
                                proto: p.proto.clone(),
                            })
                        } else {
                            None
                        }
                    })
                })
                .collect();

            PortProxyRule {
                listen_addr: la,
                listen_port: lp,
                connect_addr: ca,
                connect_port: cp,
                win_open,
                wsl_running,
                docker_matches,
            }
        })
        .collect();

    let mut wsl_ports_vec: Vec<u16> = wsl_ports.into_iter().collect();
    wsl_ports_vec.sort_unstable();

    AllData {
        rules,
        wsl_ports: wsl_ports_vec,
        docker_containers,
        errors,
        debug_log: wsl_log,
    }
}

#[tauri::command]
async fn add_rule(la: String, lp: String, ca: String, cp: String) -> Result<(), String> {
    exec_checked(&[
        "netsh",
        "interface",
        "portproxy",
        "add",
        "v4tov4",
        &format!("listenaddress={}", la),
        &format!("listenport={}", lp),
        &format!("connectaddress={}", ca),
        &format!("connectport={}", cp),
    ])
}

#[tauri::command]
async fn delete_rule(la: String, lp: String) -> Result<(), String> {
    exec_checked(&[
        "netsh",
        "interface",
        "portproxy",
        "delete",
        "v4tov4",
        &format!("listenaddress={}", la),
        &format!("listenport={}", lp),
    ])
}

#[tauri::command]
async fn forward_port(host_port: u16, wsl_ip: String) -> Result<(), String> {
    let p = host_port.to_string();
    add_rule("0.0.0.0".into(), p.clone(), wsl_ip, p).await
}

// ── Entry point ──────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            is_admin,
            get_wsl_ip,
            get_all_data,
            add_rule,
            delete_rule,
            forward_port,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
