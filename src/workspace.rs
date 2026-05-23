use anyhow::{anyhow, bail, Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
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
    pub xdpyinfo: Check,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceWindow {
    pub id: String,
    pub title: String,
    pub pid: Option<u32>,
    pub geometry: WindowGeometry,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub screen: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceScreenshot {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum IpcRequest {
    Status,
    LaunchApp { command: Vec<String> },
    ListWindows,
    Screenshot { output_path: Option<PathBuf> },
    FocusWindow { window_id: String },
    CloseWindow { window_id: String },
    Click { x: i32, y: i32 },
    Key { key: String },
    TypeText { text: String },
    KillApp { app_id: String },
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IpcResponse {
    pub ok: bool,
    pub message: String,
    pub status: Option<WorkspaceStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<Vec<WorkspaceWindow>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<WorkspaceScreenshot>,
}

pub fn default_workspace_id() -> String {
    DEFAULT_WORKSPACE_ID.to_string()
}

pub fn doctor_report() -> DoctorReport {
    let runtime = RuntimeReport {
        xvfb: command_path_check("Xvfb"),
        xephyr: command_path_check("Xephyr"),
        xauth: command_path_check("xauth"),
        xdpyinfo: command_path_check("xdpyinfo"),
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
    if !runtime.xdpyinfo.ok {
        blockers.push("Install xdpyinfo so workspace display readiness can be probed.".to_string());
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
    match prepare_workspace_start(options)? {
        WorkspaceStartPlan::AlreadyRunning(status) => Ok(IpcResponse {
            ok: true,
            message: format!("workspace {:?} is already running", status.id),
            status: Some(status),
            windows: None,
            screenshot: None,
        }),
        WorkspaceStartPlan::Start(daemon_options) => {
            spawn_detached_daemon(&daemon_options)?;
            wait_for_socket(&daemon_options.socket_path)?;
            request(&daemon_options.socket_path, IpcRequest::Status)
        }
    }
}

pub fn start_workspace_foreground(options: WorkspaceStartOptions) -> Result<()> {
    match prepare_workspace_start(options)? {
        WorkspaceStartPlan::AlreadyRunning(status) => {
            bail!(
                "workspace {:?} is already running on {}",
                status.id,
                status.display
            )
        }
        WorkspaceStartPlan::Start(daemon_options) => run_daemon(daemon_options),
    }
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

pub fn list_windows(id: &str) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    request(&workspace_socket_path(&id), IpcRequest::ListWindows)
}

pub fn screenshot(id: &str, output_path: Option<PathBuf>) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::Screenshot { output_path },
    )
}

pub fn focus_window(id: &str, window_id: String) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    let window_id = sanitize_x11_id(&window_id, "window id")?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::FocusWindow { window_id },
    )
}

pub fn close_window(id: &str, window_id: String) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    let window_id = sanitize_x11_id(&window_id, "window id")?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::CloseWindow { window_id },
    )
}

pub fn click(id: &str, x: i32, y: i32) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    request(&workspace_socket_path(&id), IpcRequest::Click { x, y })
}

pub fn key(id: &str, key: String) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    if key.trim().is_empty() {
        bail!("key cannot be empty");
    }
    request(&workspace_socket_path(&id), IpcRequest::Key { key })
}

pub fn type_text(id: &str, text: String) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    if text.is_empty() {
        bail!("text cannot be empty");
    }
    request(&workspace_socket_path(&id), IpcRequest::TypeText { text })
}

pub fn kill_app(id: &str, app_id: String) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    if app_id.trim().is_empty() {
        bail!("app id cannot be empty");
    }
    request(&workspace_socket_path(&id), IpcRequest::KillApp { app_id })
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

    eprintln!(
        "agent workspace daemon listening on {} for display {}",
        state.status.socket_path.display(),
        state.status.display
    );
    loop {
        let stream = match listener.accept() {
            Ok((stream, _addr)) => stream,
            Err(error) => {
                eprintln!("workspace IPC accept failed: {error}");
                continue;
            }
        };
        let should_stop = match handle_stream(stream, &mut state) {
            Ok(should_stop) => should_stop,
            Err(error) => {
                eprintln!("workspace IPC request failed: {error:#}");
                false
            }
        };
        if should_stop {
            break;
        }
    }

    eprintln!("agent workspace daemon stopping");
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

enum WorkspaceStartPlan {
    AlreadyRunning(WorkspaceStatus),
    Start(DaemonOptions),
}

fn prepare_workspace_start(options: WorkspaceStartOptions) -> Result<WorkspaceStartPlan> {
    let id = sanitize_workspace_id(&options.id)?;
    if let Ok(status) = status_workspace(&id) {
        return Ok(WorkspaceStartPlan::AlreadyRunning(status));
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

    Ok(WorkspaceStartPlan::Start(DaemonOptions {
        id,
        display,
        width: options.width,
        height: options.height,
        runtime_dir,
        socket_path,
        xauthority_path,
    }))
}

fn spawn_detached_daemon(options: &DaemonOptions) -> Result<()> {
    let stdout_path = options.runtime_dir.join("daemon.out.log");
    let stderr_path = options.runtime_dir.join("daemon.err.log");
    let exe = env::current_exe().context("failed to resolve current executable")?;
    let mut daemon = Command::new("setsid");
    daemon
        .arg(exe)
        .arg("daemon")
        .arg("--id")
        .arg(&options.id)
        .arg("--display")
        .arg(&options.display)
        .arg("--width")
        .arg(options.width.to_string())
        .arg("--height")
        .arg(options.height.to_string())
        .arg("--runtime-dir")
        .arg(&options.runtime_dir)
        .arg("--socket")
        .arg(&options.socket_path)
        .arg("--xauthority")
        .arg(&options.xauthority_path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(fs::File::create(&stdout_path).with_context(
            || format!("failed to create {}", stdout_path.display()),
        )?))
        .stderr(Stdio::from(fs::File::create(&stderr_path).with_context(
            || format!("failed to create {}", stderr_path.display()),
        )?));

    daemon
        .spawn()
        .context("failed to spawn agent workspace daemon")?;
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
                windows: None,
                screenshot: None,
            },
            false,
        ),
        IpcRequest::LaunchApp { command } => match spawn_app(state, command) {
            Ok(()) => (
                IpcResponse {
                    ok: true,
                    message: "app launched in workspace".to_string(),
                    status: Some(state.status.clone()),
                    windows: None,
                    screenshot: None,
                },
                false,
            ),
            Err(error) => (
                IpcResponse {
                    ok: false,
                    message: error.to_string(),
                    status: Some(state.status.clone()),
                    windows: None,
                    screenshot: None,
                },
                false,
            ),
        },
        IpcRequest::ListWindows => match list_workspace_windows(&state.status) {
            Ok(windows) => (
                IpcResponse {
                    ok: true,
                    message: "workspace windows listed".to_string(),
                    status: Some(state.status.clone()),
                    windows: Some(windows),
                    screenshot: None,
                },
                false,
            ),
            Err(error) => (
                IpcResponse {
                    ok: false,
                    message: error.to_string(),
                    status: Some(state.status.clone()),
                    windows: None,
                    screenshot: None,
                },
                false,
            ),
        },
        IpcRequest::Screenshot { output_path } => {
            match capture_workspace_screenshot(&state.status, output_path) {
                Ok(screenshot) => (
                    IpcResponse {
                        ok: true,
                        message: "workspace screenshot captured".to_string(),
                        status: Some(state.status.clone()),
                        windows: None,
                        screenshot: Some(screenshot),
                    },
                    false,
                ),
                Err(error) => (
                    IpcResponse {
                        ok: false,
                        message: error.to_string(),
                        status: Some(state.status.clone()),
                        windows: None,
                        screenshot: None,
                    },
                    false,
                ),
            }
        }
        IpcRequest::FocusWindow { window_id } => {
            match focus_workspace_window(&state.status, &window_id) {
                Ok(()) => (
                    IpcResponse {
                        ok: true,
                        message: "workspace window focused".to_string(),
                        status: Some(state.status.clone()),
                        windows: None,
                        screenshot: None,
                    },
                    false,
                ),
                Err(error) => (
                    IpcResponse {
                        ok: false,
                        message: error.to_string(),
                        status: Some(state.status.clone()),
                        windows: None,
                        screenshot: None,
                    },
                    false,
                ),
            }
        }
        IpcRequest::CloseWindow { window_id } => {
            match close_workspace_window(&state.status, &window_id) {
                Ok(()) => (
                    IpcResponse {
                        ok: true,
                        message: "workspace window close requested".to_string(),
                        status: Some(state.status.clone()),
                        windows: None,
                        screenshot: None,
                    },
                    false,
                ),
                Err(error) => (
                    IpcResponse {
                        ok: false,
                        message: error.to_string(),
                        status: Some(state.status.clone()),
                        windows: None,
                        screenshot: None,
                    },
                    false,
                ),
            }
        }
        IpcRequest::Click { x, y } => match click_workspace(&state.status, x, y) {
            Ok(()) => (
                IpcResponse {
                    ok: true,
                    message: "workspace click sent".to_string(),
                    status: Some(state.status.clone()),
                    windows: None,
                    screenshot: None,
                },
                false,
            ),
            Err(error) => (
                IpcResponse {
                    ok: false,
                    message: error.to_string(),
                    status: Some(state.status.clone()),
                    windows: None,
                    screenshot: None,
                },
                false,
            ),
        },
        IpcRequest::Key { key } => match key_workspace(&state.status, key) {
            Ok(()) => (
                IpcResponse {
                    ok: true,
                    message: "workspace key sent".to_string(),
                    status: Some(state.status.clone()),
                    windows: None,
                    screenshot: None,
                },
                false,
            ),
            Err(error) => (
                IpcResponse {
                    ok: false,
                    message: error.to_string(),
                    status: Some(state.status.clone()),
                    windows: None,
                    screenshot: None,
                },
                false,
            ),
        },
        IpcRequest::TypeText { text } => match type_workspace_text(&state.status, text) {
            Ok(()) => (
                IpcResponse {
                    ok: true,
                    message: "workspace text typed".to_string(),
                    status: Some(state.status.clone()),
                    windows: None,
                    screenshot: None,
                },
                false,
            ),
            Err(error) => (
                IpcResponse {
                    ok: false,
                    message: error.to_string(),
                    status: Some(state.status.clone()),
                    windows: None,
                    screenshot: None,
                },
                false,
            ),
        },
        IpcRequest::KillApp { app_id } => match kill_workspace_app(state, &app_id) {
            Ok(message) => (
                IpcResponse {
                    ok: true,
                    message,
                    status: Some(state.status.clone()),
                    windows: None,
                    screenshot: None,
                },
                false,
            ),
            Err(error) => (
                IpcResponse {
                    ok: false,
                    message: error.to_string(),
                    status: Some(state.status.clone()),
                    windows: None,
                    screenshot: None,
                },
                false,
            ),
        },
        IpcRequest::Stop => (
            IpcResponse {
                ok: true,
                message: "workspace stopping".to_string(),
                status: Some(state.status.clone()),
                windows: None,
                screenshot: None,
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

fn list_workspace_windows(status: &WorkspaceStatus) -> Result<Vec<WorkspaceWindow>> {
    let output = workspace_command(status, "xdotool")
        .args(["search", "--onlyvisible", "--name", "."])
        .output()
        .context("failed to run xdotool window search")?;
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| {
            let id = line.trim();
            (!id.is_empty()).then(|| window_info(status, id))
        })
        .collect()
}

fn window_info(status: &WorkspaceStatus, id: &str) -> Result<WorkspaceWindow> {
    let title = workspace_command(status, "xdotool")
        .args(["getwindowname", id])
        .output()
        .with_context(|| format!("failed to read window name for {id}"))
        .and_then(|output| output_text(output, "xdotool getwindowname"))
        .unwrap_or_default()
        .trim()
        .to_string();
    let pid = workspace_command(status, "xdotool")
        .args(["getwindowpid", id])
        .output()
        .ok()
        .and_then(|output| output.status.success().then_some(output.stdout))
        .and_then(|stdout| String::from_utf8(stdout).ok())
        .and_then(|text| text.trim().parse::<u32>().ok());
    let geometry_output = workspace_command(status, "xdotool")
        .args(["getwindowgeometry", "--shell", id])
        .output()
        .with_context(|| format!("failed to read window geometry for {id}"))?;
    let geometry_text = output_text(geometry_output, "xdotool getwindowgeometry")?;

    Ok(WorkspaceWindow {
        id: id.to_string(),
        title,
        pid,
        geometry: parse_window_geometry(&geometry_text)?,
    })
}

fn parse_window_geometry(text: &str) -> Result<WindowGeometry> {
    let mut x = None;
    let mut y = None;
    let mut width = None;
    let mut height = None;
    let mut screen = None;

    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "X" => x = value.parse::<i32>().ok(),
            "Y" => y = value.parse::<i32>().ok(),
            "WIDTH" => width = value.parse::<u32>().ok(),
            "HEIGHT" => height = value.parse::<u32>().ok(),
            "SCREEN" => screen = value.parse::<i32>().ok(),
            _ => {}
        }
    }

    Ok(WindowGeometry {
        x: x.context("window geometry missing X")?,
        y: y.context("window geometry missing Y")?,
        width: width.context("window geometry missing WIDTH")?,
        height: height.context("window geometry missing HEIGHT")?,
        screen,
    })
}

fn capture_workspace_screenshot(
    status: &WorkspaceStatus,
    output_path: Option<PathBuf>,
) -> Result<WorkspaceScreenshot> {
    let path = resolve_screenshot_path(status, output_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    if command_path_check("import").ok {
        let output = workspace_command(status, "import")
            .args(["-window", "root"])
            .arg(&path)
            .output()
            .context("failed to run import for workspace screenshot")?;
        output_text(output, "import -window root")?;
    } else if command_path_check("scrot").ok {
        let output = workspace_command(status, "scrot")
            .arg(&path)
            .output()
            .context("failed to run scrot for workspace screenshot")?;
        output_text(output, "scrot")?;
    } else {
        bail!("missing screenshot command: install ImageMagick import or scrot");
    }

    Ok(WorkspaceScreenshot {
        path,
        width: status.width,
        height: status.height,
        format: "png".to_string(),
    })
}

fn resolve_screenshot_path(status: &WorkspaceStatus, output_path: Option<PathBuf>) -> PathBuf {
    match output_path {
        Some(path) if path.is_absolute() => path,
        Some(path) => status.runtime_dir.join(path),
        None => status
            .runtime_dir
            .join(format!("screenshot-{}.png", unix_now())),
    }
}

fn focus_workspace_window(status: &WorkspaceStatus, window_id: &str) -> Result<()> {
    let window_id = sanitize_x11_id(window_id, "window id")?;
    let output = workspace_command(status, "xdotool")
        .args(["windowactivate", "--sync", &window_id])
        .output()
        .context("failed to run xdotool windowactivate")?;
    output_text(output, "xdotool windowactivate")?;
    Ok(())
}

fn close_workspace_window(status: &WorkspaceStatus, window_id: &str) -> Result<()> {
    let window_id = sanitize_x11_id(window_id, "window id")?;
    let output = workspace_command(status, "xdotool")
        .args(["windowclose", &window_id])
        .output()
        .context("failed to run xdotool windowclose")?;
    output_text(output, "xdotool windowclose")?;
    Ok(())
}

fn click_workspace(status: &WorkspaceStatus, x: i32, y: i32) -> Result<()> {
    if x < 0 || y < 0 || x as u32 >= status.width || y as u32 >= status.height {
        bail!(
            "click coordinates {x},{y} are outside workspace bounds {}x{}",
            status.width,
            status.height
        );
    }
    let output = workspace_command(status, "xdotool")
        .args(["mousemove", "--sync", &x.to_string(), &y.to_string()])
        .args(["click", "1"])
        .output()
        .context("failed to run xdotool click")?;
    output_text(output, "xdotool click")?;
    Ok(())
}

fn key_workspace(status: &WorkspaceStatus, key: String) -> Result<()> {
    if key.trim().is_empty() {
        bail!("key cannot be empty");
    }
    let output = workspace_command(status, "xdotool")
        .args(["key", "--clearmodifiers", key.trim()])
        .output()
        .context("failed to run xdotool key")?;
    output_text(output, "xdotool key")?;
    Ok(())
}

fn type_workspace_text(status: &WorkspaceStatus, text: String) -> Result<()> {
    if text.is_empty() {
        bail!("text cannot be empty");
    }
    let output = workspace_command(status, "xdotool")
        .args(["type", "--clearmodifiers", "--delay", "1", &text])
        .output()
        .context("failed to run xdotool type")?;
    output_text(output, "xdotool type")?;
    Ok(())
}

fn kill_workspace_app(state: &mut DaemonState, app_id: &str) -> Result<String> {
    let app_id = app_id.trim();
    if app_id.is_empty() {
        bail!("app id cannot be empty");
    }

    let message = {
        let app = state
            .apps
            .iter_mut()
            .find(|app| matches_app_id(&app.info, app_id))
            .ok_or_else(|| anyhow!("workspace app {app_id:?} was not found"))?;

        if !app.info.running {
            format!("workspace app {} is already stopped", app.info.id)
        } else if let Some(status) = app
            .child
            .try_wait()
            .context("failed to check app process status")?
        {
            app.info.running = false;
            app.info.exit_status = Some(status.to_string());
            format!("workspace app {} is already stopped", app.info.id)
        } else {
            app.child
                .kill()
                .with_context(|| format!("failed to kill workspace app {}", app.info.id))?;
            let status = app
                .child
                .wait()
                .with_context(|| format!("failed to wait for workspace app {}", app.info.id))?;
            app.info.running = false;
            app.info.exit_status = Some(status.to_string());
            format!("workspace app {} killed", app.info.id)
        }
    };

    state.status.apps = state.apps.iter().map(|app| app.info.clone()).collect();
    Ok(message)
}

fn matches_app_id(app: &WorkspaceApp, app_id: &str) -> bool {
    app.id == app_id || app.pid.to_string() == app_id
}

fn workspace_command(status: &WorkspaceStatus, program: &str) -> Command {
    let mut command = Command::new(program);
    command
        .env("DISPLAY", &status.display)
        .env("XAUTHORITY", &status.xauthority_path)
        .stdin(Stdio::null());
    command
}

fn output_text(output: std::process::Output, description: &str) -> Result<String> {
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        bail!("{description} failed: {detail}");
    }
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
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
    let mut file = fs::File::open("/dev/urandom").context("failed to open /dev/urandom")?;
    let mut bytes = [0_u8; 16];
    file.read_exact(&mut bytes)
        .context("failed to read random X authority cookie")?;
    Ok(bytes
        .into_iter()
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

fn sanitize_x11_id(id: &str, label: &str) -> Result<String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        bail!("{label} cannot be empty");
    }
    if !trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        bail!("{label} must be a decimal X11 id");
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
