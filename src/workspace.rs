use anyhow::{anyhow, bail, Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_WORKSPACE_ID: &str = "default";
const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 720;
const DISPLAY_RANGE: std::ops::Range<u32> = 90..180;

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DoctorReport {
    pub runtime: RuntimeReport,
    pub ready_for_x11_workspace: bool,
    pub blockers: Vec<String>,
    pub recommended_next_step: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RuntimeReport {
    pub xvfb: Check,
    pub xephyr: Check,
    pub xauth: Check,
    pub window_manager: Check,
    pub xdotool: Check,
    pub screenshot: Check,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Check {
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct WorkspaceStartOptions {
    pub id: String,
    pub width: u32,
    pub height: u32,
}

impl Default for WorkspaceStartOptions {
    fn default() -> Self {
        Self {
            id: DEFAULT_WORKSPACE_ID.to_string(),
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DaemonOptions {
    pub id: String,
    pub display: String,
    pub width: u32,
    pub height: u32,
    pub runtime_dir: PathBuf,
    pub socket_path: PathBuf,
    pub xauthority_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceStatus {
    pub id: String,
    pub ready: bool,
    pub display: String,
    pub width: u32,
    pub height: u32,
    pub runtime_dir: PathBuf,
    pub socket_path: PathBuf,
    pub xauthority_path: PathBuf,
    pub x_server_pid: u32,
    pub window_manager_pid: Option<u32>,
    pub apps: Vec<WorkspaceApp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceApp {
    pub id: String,
    pub pid: u32,
    pub command: Vec<String>,
    pub started_at_unix: u64,
    pub running: bool,
    pub exit_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum IpcRequest {
    Status,
    LaunchApp { command: Vec<String> },
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IpcResponse {
    pub ok: bool,
    pub message: String,
    pub status: Option<WorkspaceStatus>,
}

pub fn default_workspace_id() -> String {
    DEFAULT_WORKSPACE_ID.to_string()
}

pub fn doctor_report() -> DoctorReport {
    let runtime = RuntimeReport {
        xvfb: command_path_check("Xvfb"),
        xephyr: command_path_check("Xephyr"),
        xauth: command_path_check("xauth"),
        window_manager: first_available_command(&["openbox", "i3", "fluxbox"]),
        xdotool: command_path_check("xdotool"),
        screenshot: first_available_command(&["import", "scrot"]),
    };

    let mut blockers = Vec::new();
    if !runtime.xvfb.ok && !runtime.xephyr.ok {
        blockers.push("Install Xvfb or Xephyr to create the isolated X11 display.".to_string());
    }
    if !runtime.xauth.ok {
        blockers.push(
            "Install xauth so workspace displays can use a scoped authority file.".to_string(),
        );
    }
    if !runtime.window_manager.ok {
        blockers.push(
            "Install a lightweight window manager such as openbox, i3, or fluxbox.".to_string(),
        );
    }
    if !runtime.xdotool.ok {
        blockers.push(
            "Install xdotool for scoped input and window control inside the workspace.".to_string(),
        );
    }
    if !runtime.screenshot.ok {
        blockers.push("Install ImageMagick import or scrot for workspace screenshots.".to_string());
    }

    let ready_for_x11_workspace = blockers.is_empty();
    let recommended_next_step = if ready_for_x11_workspace {
        "Run `agent-workspace-linux workspace start`, then launch apps into the workspace."
            .to_string()
    } else {
        "Install the missing X11 workspace dependencies, then rerun `agent-workspace-linux doctor`."
            .to_string()
    };

    DoctorReport {
        runtime,
        ready_for_x11_workspace,
        blockers,
        recommended_next_step,
    }
}

pub fn start_workspace(options: WorkspaceStartOptions) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(&options.id)?;
    if let Ok(status) = status_workspace(&id) {
        return Ok(IpcResponse {
            ok: true,
            message: format!("workspace {id:?} is already running"),
            status: Some(status),
        });
    }

    let runtime = doctor_report();
    if !runtime.ready_for_x11_workspace {
        bail!(
            "workspace runtime is not ready: {}",
            runtime.blockers.join("; ")
        );
    }

    let runtime_dir = workspace_dir(&id);
    fs::create_dir_all(&runtime_dir)
        .with_context(|| format!("failed to create {}", runtime_dir.display()))?;
    let socket_path = runtime_dir.join("control.sock");
    remove_stale_socket(&socket_path)?;
    let xauthority_path = runtime_dir.join("Xauthority");
    let display = pick_display()?;
    create_xauthority(&display, &xauthority_path)?;

    let exe = env::current_exe().context("failed to resolve current executable")?;
    let mut daemon = Command::new(exe);
    daemon
        .arg("daemon")
        .arg("--id")
        .arg(&id)
        .arg("--display")
        .arg(&display)
        .arg("--width")
        .arg(options.width.to_string())
        .arg("--height")
        .arg(options.height.to_string())
        .arg("--runtime-dir")
        .arg(&runtime_dir)
        .arg("--socket")
        .arg(&socket_path)
        .arg("--xauthority")
        .arg(&xauthority_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    daemon
        .spawn()
        .context("failed to spawn agent workspace daemon")?;
    wait_for_socket(&socket_path)?;
    request(&socket_path, IpcRequest::Status)
}

pub fn status_workspace(id: &str) -> Result<WorkspaceStatus> {
    let id = sanitize_workspace_id(id)?;
    let response = request(&workspace_socket_path(&id), IpcRequest::Status)?;
    response
        .status
        .ok_or_else(|| anyhow!("workspace daemon returned no status"))
}

pub fn launch_app(id: &str, command: Vec<String>) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    if command.is_empty() {
        bail!("launch command cannot be empty");
    }
    request(
        &workspace_socket_path(&id),
        IpcRequest::LaunchApp { command },
    )
}

pub fn stop_workspace(id: &str) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    request(&workspace_socket_path(&id), IpcRequest::Stop)
}

pub fn run_daemon(options: DaemonOptions) -> Result<()> {
    let id = sanitize_workspace_id(&options.id)?;
    fs::create_dir_all(&options.runtime_dir)
        .with_context(|| format!("failed to create {}", options.runtime_dir.display()))?;
    remove_stale_socket(&options.socket_path)?;

    let mut x_server = spawn_xvfb(&options)?;
    wait_for_display(&options.display, &options.xauthority_path)?;
    let mut window_manager = spawn_window_manager(&options)?;

    let listener = UnixListener::bind(&options.socket_path)
        .with_context(|| format!("failed to bind {}", options.socket_path.display()))?;
    let mut state = DaemonState {
        status: WorkspaceStatus {
            id,
            ready: true,
            display: options.display,
            width: options.width,
            height: options.height,
            runtime_dir: options.runtime_dir,
            socket_path: options.socket_path,
            xauthority_path: options.xauthority_path,
            x_server_pid: x_server.id(),
            window_manager_pid: window_manager.as_ref().map(Child::id),
            apps: Vec::new(),
        },
        apps: Vec::new(),
    };

    for stream in listener.incoming() {
        let stream = stream.context("failed to accept workspace IPC connection")?;
        let should_stop = handle_stream(stream, &mut state)?;
        if should_stop {
            break;
        }
    }

    for app in &mut state.apps {
        let _ = app.child.kill();
        let _ = app.child.wait();
    }
    if let Some(wm) = &mut window_manager {
        let _ = wm.kill();
        let _ = wm.wait();
    }
    let _ = x_server.kill();
    let _ = x_server.wait();
    let _ = fs::remove_file(&state.status.socket_path);
    Ok(())
}

struct DaemonState {
    status: WorkspaceStatus,
    apps: Vec<AppProcess>,
}

struct AppProcess {
    info: WorkspaceApp,
    child: Child,
}

fn handle_stream(mut stream: UnixStream, state: &mut DaemonState) -> Result<bool> {
    let mut line = String::new();
    {
        let mut reader = BufReader::new(&stream);
        reader.read_line(&mut line)?;
    }
    let request: IpcRequest =
        serde_json::from_str(line.trim()).context("failed to parse workspace IPC request")?;
    refresh_apps(state);

    let (response, should_stop) = match request {
        IpcRequest::Status => (
            IpcResponse {
                ok: true,
                message: "workspace is running".to_string(),
                status: Some(state.status.clone()),
            },
            false,
        ),
        IpcRequest::LaunchApp { command } => match spawn_app(state, command) {
            Ok(()) => (
                IpcResponse {
                    ok: true,
                    message: "app launched in workspace".to_string(),
                    status: Some(state.status.clone()),
                },
                false,
            ),
            Err(error) => (
                IpcResponse {
                    ok: false,
                    message: error.to_string(),
                    status: Some(state.status.clone()),
                },
                false,
            ),
        },
        IpcRequest::Stop => (
            IpcResponse {
                ok: true,
                message: "workspace stopping".to_string(),
                status: Some(state.status.clone()),
            },
            true,
        ),
    };

    serde_json::to_writer(&mut stream, &response)?;
    stream.write_all(b"\n")?;
    Ok(should_stop)
}

fn spawn_app(state: &mut DaemonState, command: Vec<String>) -> Result<()> {
    if command.is_empty() {
        bail!("launch command cannot be empty");
    }
    let mut child_command = Command::new(&command[0]);
    child_command.args(&command[1..]);
    child_command
        .env("DISPLAY", &state.status.display)
        .env("XAUTHORITY", &state.status.xauthority_path)
        .stdin(Stdio::null());
    let child = child_command
        .spawn()
        .with_context(|| format!("failed to launch {}", command.join(" ")))?;
    let pid = child.id();
    let info = WorkspaceApp {
        id: format!("app-{pid}"),
        pid,
        command,
        started_at_unix: unix_now(),
        running: true,
        exit_status: None,
    };
    state.status.apps.push(info.clone());
    state.apps.push(AppProcess { info, child });
    Ok(())
}

fn refresh_apps(state: &mut DaemonState) {
    for app in &mut state.apps {
        if app.info.running {
            match app.child.try_wait() {
                Ok(Some(status)) => {
                    app.info.running = false;
                    app.info.exit_status = Some(status.to_string());
                }
                Ok(None) => {}
                Err(error) => {
                    app.info.running = false;
                    app.info.exit_status = Some(error.to_string());
                }
            }
        }
    }
    state.status.apps = state.apps.iter().map(|app| app.info.clone()).collect();
}

fn request(socket_path: &Path, request: IpcRequest) -> Result<IpcResponse> {
    let mut stream = UnixStream::connect(socket_path)
        .with_context(|| format!("failed to connect to {}", socket_path.display()))?;
    serde_json::to_writer(&mut stream, &request)?;
    stream.write_all(b"\n")?;

    let mut line = String::new();
    let mut reader = BufReader::new(stream);
    reader.read_line(&mut line)?;
    let response: IpcResponse =
        serde_json::from_str(line.trim()).context("failed to parse workspace IPC response")?;
    Ok(response)
}

fn spawn_xvfb(options: &DaemonOptions) -> Result<Child> {
    Command::new("Xvfb")
        .arg(&options.display)
        .args(["-screen", "0"])
        .arg(format!("{}x{}x24", options.width, options.height))
        .args(["-nolisten", "tcp"])
        .arg("-auth")
        .arg(&options.xauthority_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to start Xvfb")
}

fn spawn_window_manager(options: &DaemonOptions) -> Result<Option<Child>> {
    let Some(command) = first_available_command_name(&["openbox", "i3", "fluxbox"]) else {
        bail!("missing window manager: install openbox, i3, or fluxbox");
    };
    let child = Command::new(command)
        .env("DISPLAY", &options.display)
        .env("XAUTHORITY", &options.xauthority_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to start {command}"))?;
    Ok(Some(child))
}

fn wait_for_display(display: &str, xauthority: &Path) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        let ok = Command::new("xdpyinfo")
            .arg("-display")
            .arg(display)
            .env("XAUTHORITY", xauthority)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if ok {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    bail!("timed out waiting for X display {display}");
}

fn wait_for_socket(socket_path: &Path) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if socket_path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    bail!("timed out waiting for workspace IPC socket");
}

fn pick_display() -> Result<String> {
    for number in DISPLAY_RANGE {
        let display = format!(":{number}");
        let socket = PathBuf::from(format!("/tmp/.X11-unix/X{number}"));
        if socket.exists() {
            continue;
        }
        let in_use = Command::new("xdpyinfo")
            .arg("-display")
            .arg(&display)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !in_use {
            return Ok(display);
        }
    }
    bail!("no free X11 display found in range :90..:179");
}

fn create_xauthority(display: &str, path: &Path) -> Result<()> {
    let cookie = random_cookie()?;
    let _ = fs::remove_file(path);
    let output = Command::new("xauth")
        .arg("-f")
        .arg(path)
        .arg("add")
        .arg(display)
        .arg(".")
        .arg(cookie)
        .output()
        .context("failed to run xauth")?;
    if output.status.success() {
        Ok(())
    } else {
        bail!(
            "xauth failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
}

fn random_cookie() -> Result<String> {
    let bytes = fs::read("/dev/urandom").context("failed to read /dev/urandom")?;
    Ok(bytes
        .into_iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>())
}

fn remove_stale_socket(socket_path: &Path) -> Result<()> {
    if socket_path.exists() {
        fs::remove_file(socket_path)
            .with_context(|| format!("failed to remove {}", socket_path.display()))?;
    }
    Ok(())
}

fn workspace_socket_path(id: &str) -> PathBuf {
    workspace_dir(id).join("control.sock")
}

fn workspace_dir(id: &str) -> PathBuf {
    runtime_base_dir().join(id)
}

fn runtime_base_dir() -> PathBuf {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let user = env::var("USER").unwrap_or_else(|_| "user".to_string());
            PathBuf::from(format!("/tmp/agent-workspace-linux-{user}"))
        })
        .join("agent-workspace-linux")
}

fn sanitize_workspace_id(id: &str) -> Result<String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        bail!("workspace id cannot be empty");
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        bail!("workspace id may only contain ASCII letters, numbers, '-' and '_'");
    }
    Ok(trimmed.to_string())
}

fn first_available_command(commands: &[&str]) -> Check {
    for command in commands {
        let check = command_path_check(command);
        if check.ok {
            return check;
        }
    }

    Check {
        ok: false,
        detail: format!("missing all of: {}", commands.join(", ")),
    }
}

fn first_available_command_name<'a>(commands: &'a [&str]) -> Option<&'a str> {
    commands
        .iter()
        .find(|command| command_path_check(command).ok)
        .copied()
}

fn command_path_check(command: &str) -> Check {
    match Command::new("sh")
        .args(["-c", &format!("command -v {command}")])
        .output()
    {
        Ok(output) if output.status.success() => {
            let detail = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Check {
                ok: true,
                detail: if detail.is_empty() {
                    "ok".to_string()
                } else {
                    detail
                },
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let detail = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("missing: {command}")
            };
            Check { ok: false, detail }
        }
        Err(error) => Check {
            ok: false,
            detail: error.to_string(),
        },
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
