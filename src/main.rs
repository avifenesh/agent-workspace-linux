mod policy;
mod profile;
mod server;
mod workspace;

use anyhow::{bail, Context, Result};
use policy::AppliedWorkspacePolicy;
use profile::WorkspaceProfile;
use std::{fs, path::PathBuf};
use workspace::{DaemonOptions, EnvVar, LaunchSpec, WorkspaceStartOptions};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("doctor") => {
            let report = workspace::doctor_report();
            print_json(&report)
        }
        Some("mcp") => server::serve_mcp().await,
        Some("profile") => {
            args.remove(0);
            handle_profile(args)
        }
        Some("workspace") => {
            args.remove(0);
            handle_workspace(args)
        }
        Some("daemon") => {
            args.remove(0);
            workspace::run_daemon(parse_daemon_options(args)?)
        }
        Some("--help") | Some("-h") | None => {
            print_help();
            Ok(())
        }
        Some(command) => {
            bail!(
                "unknown command '{command}'. Expected one of: doctor, mcp, profile, workspace, --help"
            )
        }
    }
}

fn handle_profile(args: Vec<String>) -> Result<()> {
    let Some(command) = args.first().map(String::as_str) else {
        bail!("missing profile command. Expected: path, list, get, check, template, put, delete");
    };
    match command {
        "path" => {
            parse_no_options(&args[1..], "profile path")?;
            print_json(&profile::profile_path())
        }
        "list" => {
            parse_no_options(&args[1..], "profile list")?;
            print_json(&profile::list_profiles()?)
        }
        "get" => {
            let id = parse_required_id_arg(&args[1..], "profile get requires an id")?;
            print_json(&profile::get_profile(&id)?)
        }
        "check" => {
            let id = parse_required_id_arg(&args[1..], "profile check requires an id")?;
            print_json(&profile::check_profile(&id)?)
        }
        "template" => {
            let (kind, id, host_path) = parse_profile_template_options(&args[1..])?;
            print_json(&profile::template_profile(&kind, id, host_path)?)
        }
        "put" => {
            let profile = parse_profile_put_options(&args[1..])?;
            print_json(&profile::put_profile(profile)?)
        }
        "delete" => {
            let id = parse_required_id_arg(&args[1..], "profile delete requires an id")?;
            print_json(&profile::delete_profile(&id)?)
        }
        unknown => {
            bail!("unknown profile command '{unknown}'. Expected: path, list, get, check, template, put, delete")
        }
    }
}

fn handle_workspace(args: Vec<String>) -> Result<()> {
    let Some(command) = args.first().map(String::as_str) else {
        bail!(
            "missing workspace command. Expected: start, open-profile, list, status, launch, run, launch-profile-apps, windows, screenshot, focus-window, close-window, click, key, type, logs, wait-app, events, setup, kill-app, stop"
        );
    };
    match command {
        "start" => {
            let start = parse_start_options(&args[1..])?;
            if start.foreground {
                workspace::start_workspace_foreground(start.options)
            } else {
                print_json(&workspace::start_workspace(start.options)?)
            }
        }
        "open-profile" => {
            let (start, profile_id, open_options) = parse_open_profile_options(&args[1..])?;
            print_json(&profile::open_profile_workspace(
                start.options,
                &profile_id,
                open_options,
            )?)
        }
        "status" => {
            let id = parse_id_option(&args[1..])?;
            print_json(&workspace::status_workspace(&id)?)
        }
        "list" => {
            parse_no_options(&args[1..], "workspace list")?;
            print_json(&workspace::list_workspaces()?)
        }
        "cleanup" => {
            let id = parse_optional_id_option(&args[1..])?;
            print_json(&workspace::cleanup_stale_workspaces(id)?)
        }
        "launch" => {
            let (id, spec) = parse_launch_options(&args[1..])?;
            print_json(&workspace::launch_app_with_spec(&id, spec)?)
        }
        "run" => {
            let (id, spec, timeout_ms, tail_bytes) = parse_run_options(&args[1..])?;
            print_json(&workspace::run_app_with_spec(
                &id, spec, timeout_ms, tail_bytes,
            )?)
        }
        "launch-profile-apps" => {
            let (id, profile_id, options) = parse_profile_launch_options(&args[1..])?;
            print_json(&profile::launch_profile_startup_apps(
                &id,
                &profile_id,
                options,
            )?)
        }
        "windows" => {
            let id = parse_id_option(&args[1..])?;
            print_json(&workspace::list_windows(&id)?)
        }
        "screenshot" => {
            let (id, output_path) = parse_screenshot_options(&args[1..])?;
            print_json(&workspace::screenshot(&id, output_path)?)
        }
        "focus-window" => {
            let (id, window_id) =
                parse_one_arg_command(&args[1..], "workspace focus-window requires a window id")?;
            print_json(&workspace::focus_window(&id, window_id)?)
        }
        "close-window" => {
            let (id, window_id) =
                parse_one_arg_command(&args[1..], "workspace close-window requires a window id")?;
            print_json(&workspace::close_window(&id, window_id)?)
        }
        "click" => {
            let (id, x, y) = parse_click_options(&args[1..])?;
            print_json(&workspace::click(&id, x, y)?)
        }
        "key" => {
            let (id, key) = parse_one_arg_command(&args[1..], "workspace key requires a key")?;
            print_json(&workspace::key(&id, key)?)
        }
        "type" => {
            let (id, text) = parse_text_command(&args[1..])?;
            print_json(&workspace::type_text(&id, text)?)
        }
        "logs" => {
            let (id, app_id, stream, tail_bytes) = parse_logs_options(&args[1..])?;
            print_json(&workspace::read_app_log(&id, app_id, stream, tail_bytes)?)
        }
        "wait-app" => {
            let (id, app_id, timeout_ms) = parse_wait_app_options(&args[1..])?;
            print_json(&workspace::wait_app(&id, app_id, timeout_ms)?)
        }
        "events" => {
            let (id, tail) = parse_events_options(&args[1..])?;
            print_json(&workspace::read_events(&id, tail)?)
        }
        "setup" => {
            let (id, profile_id, options) = parse_workspace_setup_options(&args[1..])?;
            print_json(&profile::launch_profile_setup(&id, &profile_id, options)?)
        }
        "kill-app" => {
            let (id, app_id) =
                parse_one_arg_command(&args[1..], "workspace kill-app requires an app id or pid")?;
            print_json(&workspace::kill_app(&id, app_id)?)
        }
        "stop" => {
            let id = parse_id_option(&args[1..])?;
            print_json(&workspace::stop_workspace(&id)?)
        }
        unknown => {
            bail!(
                "unknown workspace command '{unknown}'. Expected: {}",
                "start, open-profile, list, cleanup, status, launch, run, launch-profile-apps, windows, screenshot, focus-window, close-window, click, key, type, logs, wait-app, events, setup, kill-app, stop"
            )
        }
    }
}

struct ParsedStartOptions {
    options: WorkspaceStartOptions,
    foreground: bool,
}

fn parse_start_options(args: &[String]) -> Result<ParsedStartOptions> {
    let mut options = WorkspaceStartOptions::default();
    let mut foreground = false;
    let mut profile_id = None;
    let mut width_explicit = false;
    let mut height_explicit = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--foreground" => {
                foreground = true;
                index += 1;
            }
            "--ack-hidden-workspace" => {
                options.user_acknowledged_hidden_workspace = true;
                index += 1;
            }
            "--ack-unenforced-policy" => {
                options.user_acknowledged_unenforced_policy = true;
                index += 1;
            }
            "--profile" => {
                profile_id = Some(value_after(args, index, "--profile")?.to_string());
                index += 2;
            }
            "--id" => {
                options.id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--width" => {
                options.width = value_after(args, index, "--width")?
                    .parse()
                    .context("--width must be a positive integer")?;
                width_explicit = true;
                index += 2;
            }
            "--height" => {
                options.height = value_after(args, index, "--height")?
                    .parse()
                    .context("--height must be a positive integer")?;
                height_explicit = true;
                index += 2;
            }
            flag => bail!("unknown workspace start option '{flag}'"),
        }
    }
    if let Some(profile_id) = &profile_id {
        profile::apply_profile_to_start_options(
            profile_id,
            &mut options,
            width_explicit,
            height_explicit,
        )?;
    }
    Ok(ParsedStartOptions {
        options,
        foreground,
    })
}

fn parse_open_profile_options(
    args: &[String],
) -> Result<(
    ParsedStartOptions,
    String,
    profile::ProfileWorkspaceOpenOptions,
)> {
    let mut options = WorkspaceStartOptions::default();
    let mut profile_id = None;
    let mut width_explicit = false;
    let mut height_explicit = false;
    let mut open_options = profile::ProfileWorkspaceOpenOptions::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--ack-hidden-workspace" => {
                options.user_acknowledged_hidden_workspace = true;
                index += 1;
            }
            "--ack-unenforced-policy" => {
                options.user_acknowledged_unenforced_policy = true;
                open_options.setup.acknowledge_unenforced_policy = true;
                open_options.startup.acknowledge_unenforced_policy = true;
                index += 1;
            }
            "--profile" => {
                profile_id = Some(value_after(args, index, "--profile")?.to_string());
                index += 2;
            }
            "--id" => {
                options.id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--width" => {
                options.width = value_after(args, index, "--width")?
                    .parse()
                    .context("--width must be a positive integer")?;
                width_explicit = true;
                index += 2;
            }
            "--height" => {
                options.height = value_after(args, index, "--height")?
                    .parse()
                    .context("--height must be a positive integer")?;
                height_explicit = true;
                index += 2;
            }
            "--setup" => {
                open_options.run_setup = true;
                index += 1;
            }
            "--setup-timeout-ms" => {
                open_options.run_setup = true;
                open_options.setup.wait = true;
                open_options.setup.timeout_ms = Some(
                    value_after(args, index, "--setup-timeout-ms")?
                        .parse()
                        .context("--setup-timeout-ms must be a non-negative integer")?,
                );
                index += 2;
            }
            flag => bail!("unknown workspace open-profile option '{flag}'"),
        }
    }
    let profile_id = profile_id.context("workspace open-profile requires --profile PROFILE")?;
    profile::apply_profile_to_start_options(
        &profile_id,
        &mut options,
        width_explicit,
        height_explicit,
    )?;
    Ok((
        ParsedStartOptions {
            options,
            foreground: false,
        },
        profile_id,
        open_options,
    ))
}

fn parse_id_option(args: &[String]) -> Result<String> {
    let mut id = workspace::default_workspace_id();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            flag => bail!("unknown workspace option '{flag}'"),
        }
    }
    Ok(id)
}

fn parse_no_options(args: &[String], command: &str) -> Result<()> {
    if let Some(arg) = args.first() {
        bail!("{command} does not accept option '{arg}'");
    }
    Ok(())
}

fn parse_required_id_arg(args: &[String], missing_message: &str) -> Result<String> {
    if args.len() != 1 {
        bail!("{missing_message}");
    }
    Ok(args[0].clone())
}

fn parse_profile_put_options(args: &[String]) -> Result<WorkspaceProfile> {
    let mut json_path = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => {
                json_path = Some(PathBuf::from(value_after(args, index, "--json")?));
                index += 2;
            }
            flag => bail!("unknown profile put option '{flag}'. Expected: --json PATH"),
        }
    }
    let json_path = json_path.context("profile put requires --json PATH")?;
    let content = fs::read_to_string(&json_path)
        .with_context(|| format!("failed to read {}", json_path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse profile JSON from {}", json_path.display()))
}

fn parse_profile_template_options(
    args: &[String],
) -> Result<(String, Option<String>, Option<PathBuf>)> {
    let kind = args
        .first()
        .context("profile template requires a kind, for example project-dev")?
        .to_string();
    let mut id = None;
    let mut host_path = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = Some(value_after(args, index, "--id")?.to_string());
                index += 2;
            }
            "--host-path" => {
                host_path = Some(PathBuf::from(value_after(args, index, "--host-path")?));
                index += 2;
            }
            flag => bail!("unknown profile template option '{flag}'"),
        }
    }
    Ok((kind, id, host_path))
}

fn parse_optional_id_option(args: &[String]) -> Result<Option<String>> {
    let mut id = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = Some(value_after(args, index, "--id")?.to_string());
                index += 2;
            }
            flag => bail!("unknown workspace option '{flag}'"),
        }
    }
    Ok(id)
}

fn parse_launch_options(args: &[String]) -> Result<(String, LaunchSpec)> {
    let mut id = workspace::default_workspace_id();
    let mut profile_id = None;
    let mut cwd = None;
    let mut cwd_explicit = false;
    let mut user_acknowledged_unenforced_policy = false;
    let mut env = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--profile" => {
                profile_id = Some(value_after(args, index, "--profile")?.to_string());
                index += 2;
            }
            "--cwd" => {
                cwd = Some(PathBuf::from(value_after(args, index, "--cwd")?));
                cwd_explicit = true;
                index += 2;
            }
            "--env" => {
                env.push(parse_env_assignment(value_after(args, index, "--env")?)?);
                index += 2;
            }
            "--ack-unenforced-policy" => {
                user_acknowledged_unenforced_policy = true;
                index += 1;
            }
            "--" => {
                let command = args[index + 1..].to_vec();
                if command.is_empty() {
                    bail!("workspace launch requires a command after --");
                }
                let mut spec = LaunchSpec {
                    command,
                    profile_id: None,
                    applied_policy: None,
                    user_acknowledged_unenforced_policy,
                    cwd,
                    env,
                };
                if let Some(profile_id) = &profile_id {
                    profile::apply_profile_to_launch_spec(profile_id, &mut spec, cwd_explicit)?;
                }
                return Ok((id, spec));
            }
            _ => {
                let command = args[index..].to_vec();
                if command.is_empty() {
                    bail!("workspace launch requires a command");
                }
                let mut spec = LaunchSpec {
                    command,
                    profile_id: None,
                    applied_policy: None,
                    user_acknowledged_unenforced_policy,
                    cwd,
                    env,
                };
                if let Some(profile_id) = &profile_id {
                    profile::apply_profile_to_launch_spec(profile_id, &mut spec, cwd_explicit)?;
                }
                return Ok((id, spec));
            }
        }
    }
    bail!("workspace launch requires a command")
}

fn parse_run_options(args: &[String]) -> Result<(String, LaunchSpec, Option<u64>, Option<u64>)> {
    let mut id = workspace::default_workspace_id();
    let mut profile_id = None;
    let mut cwd = None;
    let mut cwd_explicit = false;
    let mut user_acknowledged_unenforced_policy = false;
    let mut timeout_ms = None;
    let mut tail_bytes = None;
    let mut env = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--profile" => {
                profile_id = Some(value_after(args, index, "--profile")?.to_string());
                index += 2;
            }
            "--cwd" => {
                cwd = Some(PathBuf::from(value_after(args, index, "--cwd")?));
                cwd_explicit = true;
                index += 2;
            }
            "--env" => {
                env.push(parse_env_assignment(value_after(args, index, "--env")?)?);
                index += 2;
            }
            "--ack-unenforced-policy" => {
                user_acknowledged_unenforced_policy = true;
                index += 1;
            }
            "--timeout-ms" => {
                timeout_ms = Some(
                    value_after(args, index, "--timeout-ms")?
                        .parse()
                        .context("--timeout-ms must be a non-negative integer")?,
                );
                index += 2;
            }
            "--tail-bytes" => {
                tail_bytes = Some(
                    value_after(args, index, "--tail-bytes")?
                        .parse()
                        .context("--tail-bytes must be a non-negative integer")?,
                );
                index += 2;
            }
            "--" => {
                let command = args[index + 1..].to_vec();
                if command.is_empty() {
                    bail!("workspace run requires a command after --");
                }
                let mut spec = LaunchSpec {
                    command,
                    profile_id: None,
                    applied_policy: None,
                    user_acknowledged_unenforced_policy,
                    cwd,
                    env,
                };
                if let Some(profile_id) = &profile_id {
                    profile::apply_profile_to_launch_spec(profile_id, &mut spec, cwd_explicit)?;
                }
                return Ok((id, spec, timeout_ms, tail_bytes));
            }
            _ => {
                let command = args[index..].to_vec();
                if command.is_empty() {
                    bail!("workspace run requires a command");
                }
                let mut spec = LaunchSpec {
                    command,
                    profile_id: None,
                    applied_policy: None,
                    user_acknowledged_unenforced_policy,
                    cwd,
                    env,
                };
                if let Some(profile_id) = &profile_id {
                    profile::apply_profile_to_launch_spec(profile_id, &mut spec, cwd_explicit)?;
                }
                return Ok((id, spec, timeout_ms, tail_bytes));
            }
        }
    }
    bail!("workspace run requires a command")
}

fn parse_env_assignment(value: &str) -> Result<EnvVar> {
    let Some((name, value)) = value.split_once('=') else {
        bail!("--env requires NAME=VALUE");
    };
    if name.is_empty() {
        bail!("--env requires a non-empty variable name");
    }
    Ok(EnvVar {
        name: name.to_string(),
        value: value.to_string(),
    })
}

fn parse_screenshot_options(args: &[String]) -> Result<(String, Option<PathBuf>)> {
    let mut id = workspace::default_workspace_id();
    let mut output_path = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--output" => {
                output_path = Some(PathBuf::from(value_after(args, index, "--output")?));
                index += 2;
            }
            flag => bail!("unknown workspace screenshot option '{flag}'"),
        }
    }
    Ok((id, output_path))
}

fn parse_click_options(args: &[String]) -> Result<(String, i32, i32)> {
    let (id, values) = parse_id_and_args(args)?;
    if values.len() != 2 {
        bail!("workspace click requires X and Y coordinates");
    }
    let x = values[0].parse().context("click X must be an integer")?;
    let y = values[1].parse().context("click Y must be an integer")?;
    Ok((id, x, y))
}

fn parse_one_arg_command(args: &[String], missing_message: &str) -> Result<(String, String)> {
    let (id, values) = parse_id_and_args(args)?;
    if values.len() != 1 {
        bail!("{missing_message}");
    }
    Ok((id, values[0].clone()))
}

fn parse_text_command(args: &[String]) -> Result<(String, String)> {
    let (id, values) = parse_id_and_args(args)?;
    if values.is_empty() {
        bail!("workspace type requires text");
    }
    Ok((id, values.join(" ")))
}

fn parse_logs_options(args: &[String]) -> Result<(String, String, String, Option<u64>)> {
    let mut id = workspace::default_workspace_id();
    let mut stream = "stdout".to_string();
    let mut tail_bytes = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--stream" => {
                stream = value_after(args, index, "--stream")?.to_string();
                index += 2;
            }
            "--tail-bytes" => {
                tail_bytes = Some(
                    value_after(args, index, "--tail-bytes")?
                        .parse()
                        .context("--tail-bytes must be a positive integer")?,
                );
                index += 2;
            }
            "--" => {
                let app_id = args
                    .get(index + 1)
                    .context("workspace logs requires an app id")?
                    .to_string();
                return Ok((id, app_id, stream, tail_bytes));
            }
            _ => {
                let app_id = args[index].clone();
                return Ok((id, app_id, stream, tail_bytes));
            }
        }
    }
    bail!("workspace logs requires an app id")
}

fn parse_wait_app_options(args: &[String]) -> Result<(String, String, Option<u64>)> {
    let mut id = workspace::default_workspace_id();
    let mut timeout_ms = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--timeout-ms" => {
                timeout_ms = Some(
                    value_after(args, index, "--timeout-ms")?
                        .parse()
                        .context("--timeout-ms must be a non-negative integer")?,
                );
                index += 2;
            }
            "--" => {
                let app_id = args
                    .get(index + 1)
                    .context("workspace wait-app requires an app id")?
                    .to_string();
                return Ok((id, app_id, timeout_ms));
            }
            _ => {
                let app_id = args[index].clone();
                return Ok((id, app_id, timeout_ms));
            }
        }
    }
    bail!("workspace wait-app requires an app id")
}

fn parse_events_options(args: &[String]) -> Result<(String, Option<usize>)> {
    let mut id = workspace::default_workspace_id();
    let mut tail = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--tail" => {
                tail = Some(
                    value_after(args, index, "--tail")?
                        .parse()
                        .context("--tail must be a non-negative integer")?,
                );
                index += 2;
            }
            flag => bail!("unknown workspace events option '{flag}'"),
        }
    }
    Ok((id, tail))
}

fn parse_workspace_setup_options(
    args: &[String],
) -> Result<(String, String, profile::ProfileSetupOptions)> {
    let mut id = workspace::default_workspace_id();
    let mut profile_id = None;
    let mut options = profile::ProfileSetupOptions::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--profile" => {
                profile_id = Some(value_after(args, index, "--profile")?.to_string());
                index += 2;
            }
            "--wait" => {
                options.wait = true;
                index += 1;
            }
            "--ack-unenforced-policy" => {
                options.acknowledge_unenforced_policy = true;
                index += 1;
            }
            "--timeout-ms" => {
                options.wait = true;
                options.timeout_ms = Some(
                    value_after(args, index, "--timeout-ms")?
                        .parse()
                        .context("--timeout-ms must be a non-negative integer")?,
                );
                index += 2;
            }
            flag => bail!("unknown workspace setup option '{flag}'"),
        }
    }
    Ok((
        id,
        profile_id.context("workspace setup requires --profile PROFILE")?,
        options,
    ))
}

fn parse_profile_launch_options(
    args: &[String],
) -> Result<(String, String, profile::ProfileStartupOptions)> {
    let mut id = workspace::default_workspace_id();
    let mut profile_id = None;
    let mut options = profile::ProfileStartupOptions::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--profile" => {
                profile_id = Some(value_after(args, index, "--profile")?.to_string());
                index += 2;
            }
            "--ack-unenforced-policy" => {
                options.acknowledge_unenforced_policy = true;
                index += 1;
            }
            flag => bail!("unknown workspace launch-profile-apps option '{flag}'"),
        }
    }
    Ok((
        id,
        profile_id.context("workspace launch-profile-apps requires --profile PROFILE")?,
        options,
    ))
}

fn parse_id_and_args(args: &[String]) -> Result<(String, Vec<String>)> {
    let mut id = workspace::default_workspace_id();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--" => return Ok((id, args[index + 1..].to_vec())),
            _ => return Ok((id, args[index..].to_vec())),
        }
    }
    Ok((id, Vec::new()))
}

fn parse_daemon_options(args: Vec<String>) -> Result<DaemonOptions> {
    let mut id = None;
    let mut profile_id = None;
    let mut display = None;
    let mut width = None;
    let mut height = None;
    let mut runtime_dir = None;
    let mut socket_path = None;
    let mut xauthority_path = None;
    let mut policy_path = None;
    let mut user_acknowledged_hidden_workspace = false;
    let mut user_acknowledged_unenforced_policy = false;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = Some(value_after(&args, index, "--id")?.to_string());
                index += 2;
            }
            "--profile" => {
                profile_id = Some(value_after(&args, index, "--profile")?.to_string());
                index += 2;
            }
            "--display" => {
                display = Some(value_after(&args, index, "--display")?.to_string());
                index += 2;
            }
            "--width" => {
                width = Some(
                    value_after(&args, index, "--width")?
                        .parse()
                        .context("--width must be a positive integer")?,
                );
                index += 2;
            }
            "--height" => {
                height = Some(
                    value_after(&args, index, "--height")?
                        .parse()
                        .context("--height must be a positive integer")?,
                );
                index += 2;
            }
            "--runtime-dir" => {
                runtime_dir = Some(PathBuf::from(value_after(&args, index, "--runtime-dir")?));
                index += 2;
            }
            "--socket" => {
                socket_path = Some(PathBuf::from(value_after(&args, index, "--socket")?));
                index += 2;
            }
            "--xauthority" => {
                xauthority_path = Some(PathBuf::from(value_after(&args, index, "--xauthority")?));
                index += 2;
            }
            "--policy" => {
                policy_path = Some(PathBuf::from(value_after(&args, index, "--policy")?));
                index += 2;
            }
            "--ack-hidden-workspace" => {
                user_acknowledged_hidden_workspace = true;
                index += 1;
            }
            "--ack-unenforced-policy" => {
                user_acknowledged_unenforced_policy = true;
                index += 1;
            }
            flag => bail!("unknown daemon option '{flag}'"),
        }
    }
    let applied_policy = policy_path.as_ref().map(read_applied_policy).transpose()?;

    Ok(DaemonOptions {
        id: id.context("daemon missing --id")?,
        profile_id,
        applied_policy,
        user_acknowledged_hidden_workspace,
        user_acknowledged_unenforced_policy,
        display: display.context("daemon missing --display")?,
        width: width.context("daemon missing --width")?,
        height: height.context("daemon missing --height")?,
        runtime_dir: runtime_dir.context("daemon missing --runtime-dir")?,
        socket_path: socket_path.context("daemon missing --socket")?,
        xauthority_path: xauthority_path.context("daemon missing --xauthority")?,
    })
}

fn read_applied_policy(path: &PathBuf) -> Result<AppliedWorkspacePolicy> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read applied policy {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse applied policy {}", path.display()))
}

fn value_after<'a>(args: &'a [String], index: usize, flag: &str) -> Result<&'a str> {
    args.get(index + 1)
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("{flag} requires a value"))
}

fn print_json(value: &impl serde::Serialize) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).context("failed to serialize JSON")?
    );
    Ok(())
}

fn print_help() {
    println!(
        "agent-workspace-linux\n\nUsage:\n  agent-workspace-linux doctor\n  agent-workspace-linux mcp\n  agent-workspace-linux profile path|list|get|check|template|put|delete\n  agent-workspace-linux profile template project-dev [--id ID] [--host-path PATH]\n  agent-workspace-linux workspace start --ack-hidden-workspace [--ack-unenforced-policy] [--foreground] [--profile PROFILE] [--id ID] [--width PX] [--height PX]\n  agent-workspace-linux workspace open-profile --ack-hidden-workspace [--ack-unenforced-policy] --profile PROFILE [--setup] [--setup-timeout-ms N] [--id ID] [--width PX] [--height PX]\n  agent-workspace-linux workspace list\n  agent-workspace-linux workspace cleanup [--id ID]\n  agent-workspace-linux workspace status [--id ID]\n  agent-workspace-linux workspace launch [--id ID] [--profile PROFILE] [--ack-unenforced-policy] [--cwd DIR] [--env NAME=VALUE] -- COMMAND [ARGS...]\n  agent-workspace-linux workspace run [--id ID] [--profile PROFILE] [--timeout-ms N] [--tail-bytes N] -- COMMAND [ARGS...]\n  agent-workspace-linux workspace launch-profile-apps [--id ID] --profile PROFILE [--ack-unenforced-policy]\n  agent-workspace-linux workspace windows [--id ID]\n  agent-workspace-linux workspace screenshot [--id ID] [--output PATH]\n  agent-workspace-linux workspace focus-window [--id ID] WINDOW_ID\n  agent-workspace-linux workspace close-window [--id ID] WINDOW_ID\n  agent-workspace-linux workspace click [--id ID] X Y\n  agent-workspace-linux workspace key [--id ID] KEY\n  agent-workspace-linux workspace type [--id ID] TEXT\n  agent-workspace-linux workspace logs [--id ID] [--stream stdout|stderr] [--tail-bytes N] APP_ID_OR_PID\n  agent-workspace-linux workspace wait-app [--id ID] [--timeout-ms N] APP_ID_OR_PID\n  agent-workspace-linux workspace events [--id ID] [--tail N]\n  agent-workspace-linux workspace setup [--id ID] --profile PROFILE [--wait] [--timeout-ms N] [--ack-unenforced-policy]\n  agent-workspace-linux workspace kill-app [--id ID] APP_ID_OR_PID\n  agent-workspace-linux workspace stop [--id ID]"
    );
}
