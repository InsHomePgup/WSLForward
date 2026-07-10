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
    pub firewall_open: bool,
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

// PowerShell's NetSecurity cmdlets return .NET enum/property values (always
// English), unlike `netsh advfirewall` text output, which Windows localizes
// to the OS display language — so this is parsed instead of netsh's output.
fn parse_firewall_ports(raw: &str) -> Vec<(String, String)> {
    raw.lines()
        .filter_map(|l| l.split_once('|'))
        .map(|(protocol, local_port)| (protocol.trim().to_string(), local_port.trim().to_string()))
        .collect()
}

fn local_port_matches(spec: &str, port: u16) -> bool {
    spec.split(',').any(|tok| {
        let tok = tok.trim();
        if let Some((s, e)) = tok.split_once('-') {
            match (s.trim().parse::<u16>(), e.trim().parse::<u16>()) {
                (Ok(s), Ok(e)) => port >= s && port <= e,
                _ => false,
            }
        } else {
            tok.parse::<u16>().map(|p| p == port).unwrap_or(false)
        }
    })
}

// A rule with LocalPort=Any is never treated as opening our specific target port,
// even when Get-NetFirewallApplicationFilter reports Program as "Any" or "System".
// Verified against a live system: LocalPort=Any/Program=System rules (e.g. WFD
// service discovery, IGMP, IPv6 core networking) and LocalPort=Any/Program=Any
// rules (UWP app-package rules, scoped by a Package SID that isn't exposed via
// the Program field) both leave arbitrary TCP ports genuinely closed in practice
// — confirmed empirically, since LAN access to a port stayed blocked despite
// several such rules already being enabled. So only an exact/range numeric
// LocalPort match is trusted as evidence the target port is actually open.
fn firewall_port_open(rules: &[(String, String)], port: u16) -> bool {
    rules.iter().any(|(protocol, local_port)| {
        (protocol.eq_ignore_ascii_case("tcp") || protocol.eq_ignore_ascii_case("any"))
            && local_port_matches(local_port, port)
    })
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

const FIREWALL_LIST_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
try {
    Get-NetFirewallRule -Direction Inbound | Where-Object { $_.Enabled -eq 'True' -and $_.Action -eq 'Allow' } | ForEach-Object { $_ | Get-NetFirewallPortFilter } | ForEach-Object { "$($_.Protocol)|$($_.LocalPort -join ',')" }
} catch {
    Write-Error $_
    exit 1
}
"#;

fn gather_firewall_rules() -> (Vec<(String, String)>, Option<String>) {
    match exec(&["powershell", "-NoProfile", "-NonInteractive", "-Command", FIREWALL_LIST_SCRIPT]) {
        Some(out) => (parse_firewall_ports(&out), None),
        None => (Vec::new(), Some("Get-NetFirewallRule failed — could not read firewall rules".to_string())),
    }
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
    .and_then(|s| s.lines().find(|l| !l.trim().is_empty()).map(|l| l.trim().to_string()))
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

    let (fw_rules, fw_error) = gather_firewall_rules();
    if let Some(e) = fw_error {
        errors.push(e);
    }

    let rules = raw_rules
        .into_iter()
        .map(|(la, lp, ca, cp)| {
            let check_host = if la == "0.0.0.0" || la == "*" {
                "127.0.0.1"
            } else {
                &la
            };
            let lp_u16: u16 = lp.parse().unwrap_or(0);
            let win_open = lp.parse::<u16>().map(|p| port_open(check_host, p)).unwrap_or(false);
            let firewall_open = firewall_port_open(&fw_rules, lp_u16);
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
                firewall_open,
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

fn firewall_rule_name(port: u16) -> String {
    format!("WSLForward-{}", port)
}

fn run_firewall_script(script: &str) -> Result<(), String> {
    exec_checked(&["powershell", "-NoProfile", "-NonInteractive", "-Command", script])
}

#[tauri::command]
async fn add_firewall_rule(port: String) -> Result<(), String> {
    let port: u16 = port.parse().map_err(|_| "Invalid port".to_string())?;
    let name = firewall_rule_name(port);
    // `name`/`port` are a fixed prefix + a validated u16, so interpolating them
    // into the script below can't break out of the quoted PowerShell literals.
    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
try {{
    Remove-NetFirewallRule -Name '{name}' -ErrorAction SilentlyContinue | Out-Null
    New-NetFirewallRule -Name '{name}' -DisplayName '{name}' -Direction Inbound -Action Allow -Protocol TCP -LocalPort {port} -Profile Any | Out-Null
}} catch {{
    Write-Error $_
    exit 1
}}
"#,
        name = name,
        port = port,
    );
    run_firewall_script(&script)
}

#[tauri::command]
async fn remove_firewall_rule(port: String) -> Result<(), String> {
    let port: u16 = port.parse().map_err(|_| "Invalid port".to_string())?;
    let name = firewall_rule_name(port);
    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
try {{
    if (Get-NetFirewallRule -Name '{name}' -ErrorAction SilentlyContinue) {{
        Remove-NetFirewallRule -Name '{name}'
    }}
}} catch {{
    Write-Error $_
    exit 1
}}
"#,
        name = name,
    );
    run_firewall_script(&script)
}

#[tauri::command]
async fn forward_port(host_port: u16, wsl_ip: String) -> Result<(), String> {
    let p = host_port.to_string();
    add_rule("0.0.0.0".into(), p.clone(), wsl_ip, p).await
}

#[tauri::command]
async fn restart_as_admin(app: tauri::AppHandle) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_str = exe.to_string_lossy().into_owned().replace('\'', "''");
    let ps_cmd = format!("Start-Process -FilePath '{}' -Verb RunAs", exe_str);
    let mut cmd = Command::new("powershell");
    cmd.args(["-WindowStyle", "Hidden", "-Command", &ps_cmd]);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.spawn().map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

// ── Registry watcher ─────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn start_registry_watcher(app: tauri::AppHandle) {
    use std::thread;
    use std::time::Duration;
    use tauri::Emitter;
    use windows::Win32::Foundation::{BOOL, CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_NOTIFY,
        REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME,
    };
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
    use windows::core::w;

    thread::spawn(move || {
        // Auto-reset event; used with async RegNotifyChangeKeyValue so the
        // thread is never permanently blocked and can exit when the app does.
        let event = match unsafe { CreateEventW(None, BOOL(0), BOOL(0), None) } {
            Ok(h) => h,
            Err(_) => return,
        };

        'outer: loop {
            let mut hkey = HKEY::default();
            let opened = unsafe {
                RegOpenKeyExW(
                    HKEY_LOCAL_MACHINE,
                    w!("SYSTEM\\CurrentControlSet\\Services\\PortProxy"),
                    0,
                    KEY_NOTIFY,
                    &mut hkey,
                )
                .is_ok()
            };
            if !opened {
                thread::sleep(Duration::from_secs(2));
                continue;
            }

            loop {
                // Register async notification; must re-register after each firing.
                if !unsafe {
                    RegNotifyChangeKeyValue(
                        hkey,
                        BOOL(1),
                        REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_CHANGE_NAME,
                        event,
                        BOOL(1),
                    )
                    .is_ok()
                } {
                    break;
                }

                // Wait up to 5 s; loop on timeout so the thread wakes regularly
                // and can detect app shutdown via the emit error path.
                loop {
                    let r = unsafe { WaitForSingleObject(event, 5_000) };
                    if r == WAIT_OBJECT_0 {
                        break; // change detected — re-register and emit
                    } else if r == WAIT_TIMEOUT {
                        continue; // still waiting, registration still active
                    } else {
                        break 'outer; // WAIT_FAILED or unexpected value
                    }
                }

                thread::sleep(Duration::from_millis(300));
                if app.emit("portproxy-changed", ()).is_err() {
                    break 'outer; // app is shutting down
                }
            }

            unsafe { RegCloseKey(hkey) };
            thread::sleep(Duration::from_secs(1));
        }

        unsafe { CloseHandle(event) };
    });
}

// ── Entry point ──────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|_app| {
            #[cfg(target_os = "windows")]
            start_registry_watcher(_app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            is_admin,
            get_wsl_ip,
            get_all_data,
            add_rule,
            delete_rule,
            add_firewall_rule,
            remove_firewall_rule,
            forward_port,
            restart_as_admin,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
