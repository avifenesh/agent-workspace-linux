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
            "missing workspace command. Expected: start, open-profile, list, cleanup, status, ipc-info, launch, run, launch-profile-apps, apps, windows, active-window, observe, wait-window, screenshot, screenshot-window, focus-window, close-window, move-window, resize-window, raise-window, minimize-window, show-window, click, click-window, move-pointer, move-pointer-window, drag, drag-window, scroll, scroll-window, key, key-window, type, type-window, clipboard-set, clipboard-get, paste, paste-window, logs, wait-app, events, setup, kill-app, stop"
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
        "ipc-info" => {
            let id = parse_id_option(&args[1..])?;
            print_json(&workspace::ipc_info(&id)?)
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
            let (id, spec, timeout_ms, tail_bytes, kill_on_timeout) =
                parse_run_options(&args[1..])?;
            print_json(&workspace::run_app_with_spec(
                &id,
                spec,
                timeout_ms,
                tail_bytes,
                kill_on_timeout,
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
        "apps" => {
            let parsed = parse_apps_options(&args[1..])?;
            print_json(&workspace::list_apps(
                &parsed.id,
                parsed.app_id,
                parsed.name_contains,
                parsed.command_contains,
                parsed.profile_id,
                parsed.running,
            )?)
        }
        "windows" => {
            let (id, include_hidden, title_contains, class_contains, pid, app_id) =
                parse_windows_options(&args[1..])?;
            print_json(&workspace::list_windows(
                &id,
                include_hidden,
                title_contains,
                class_contains,
                pid,
                app_id,
            )?)
        }
        "active-window" => {
            let id = parse_id_option(&args[1..])?;
            print_json(&workspace::active_window(&id)?)
        }
        "observe" => {
            let (id, screenshot, include_hidden, output_path) = parse_observe_options(&args[1..])?;
            print_json(&workspace::observe(
                &id,
                screenshot,
                include_hidden,
                output_path,
            )?)
        }
        "wait-window" => {
            let (id, title_contains, class_contains, pid, app_id, timeout_ms) =
                parse_wait_window_options(&args[1..])?;
            print_json(&workspace::wait_window(
                &id,
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            )?)
        }
        "screenshot" => {
            let (id, output_path) = parse_screenshot_options(&args[1..])?;
            print_json(&workspace::screenshot(&id, output_path)?)
        }
        "screenshot-window" => {
            let (
                id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                output_path,
                timeout_ms,
            ) = parse_screenshot_window_options(&args[1..])?;
            print_json(&workspace::screenshot_window(
                &id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                output_path,
                timeout_ms,
            )?)
        }
        "focus-window" => {
            let (id, target) = parse_focus_window_options(&args[1..])?;
            match target {
                FocusWindowTarget::WindowId(window_id) => {
                    print_json(&workspace::focus_window(&id, window_id)?)
                }
                FocusWindowTarget::Match {
                    title_contains,
                    class_contains,
                    pid,
                    app_id,
                    timeout_ms,
                } => print_json(&workspace::focus_matching_window(
                    &id,
                    title_contains,
                    class_contains,
                    pid,
                    app_id,
                    timeout_ms,
                )?),
            }
        }
        "close-window" => {
            let (id, target) = parse_close_window_options(&args[1..])?;
            match target {
                CloseWindowTarget::WindowId(window_id) => {
                    print_json(&workspace::close_window(&id, window_id)?)
                }
                CloseWindowTarget::Match {
                    title_contains,
                    class_contains,
                    pid,
                    app_id,
                    timeout_ms,
                } => print_json(&workspace::close_matching_window(
                    &id,
                    title_contains,
                    class_contains,
                    pid,
                    app_id,
                    timeout_ms,
                )?),
            }
        }
        "move-window" => {
            let (id, window_id, title_contains, class_contains, pid, app_id, x, y, timeout_ms) =
                parse_move_window_options(&args[1..])?;
            print_json(&workspace::move_window(
                &id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                x,
                y,
                timeout_ms,
            )?)
        }
        "resize-window" => {
            let (
                id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                width,
                height,
                timeout_ms,
            ) = parse_resize_window_options(&args[1..])?;
            print_json(&workspace::resize_window(
                &id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                width,
                height,
                timeout_ms,
            )?)
        }
        "raise-window" => {
            let (id, window_id, title_contains, class_contains, pid, app_id, timeout_ms) =
                parse_targeted_window_action_options(&args[1..], "workspace raise-window")?;
            print_json(&workspace::raise_window(
                &id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            )?)
        }
        "minimize-window" => {
            let (id, window_id, title_contains, class_contains, pid, app_id, timeout_ms) =
                parse_targeted_window_action_options(&args[1..], "workspace minimize-window")?;
            print_json(&workspace::minimize_window(
                &id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            )?)
        }
        "show-window" => {
            let (id, window_id, title_contains, class_contains, pid, app_id, timeout_ms) =
                parse_targeted_window_action_options(&args[1..], "workspace show-window")?;
            print_json(&workspace::show_window(
                &id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            )?)
        }
        "click" => {
            let (id, x, y, button, count) = parse_click_options(&args[1..])?;
            print_json(&workspace::click(&id, x, y, button, count)?)
        }
        "click-window" => {
            let (
                id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                x,
                y,
                button,
                count,
                timeout_ms,
            ) = parse_click_window_options(&args[1..])?;
            print_json(&workspace::click_window(
                &id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                x,
                y,
                button,
                count,
                timeout_ms,
            )?)
        }
        "move-pointer" => {
            let (id, x, y) = parse_move_pointer_options(&args[1..])?;
            print_json(&workspace::move_pointer(&id, x, y)?)
        }
        "move-pointer-window" => {
            let (id, window_id, title_contains, class_contains, pid, app_id, x, y, timeout_ms) =
                parse_move_pointer_window_options(&args[1..])?;
            print_json(&workspace::move_pointer_window(
                &id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                x,
                y,
                timeout_ms,
            )?)
        }
        "drag" => {
            let (id, from_x, from_y, to_x, to_y, button) = parse_drag_options(&args[1..])?;
            print_json(&workspace::drag(&id, from_x, from_y, to_x, to_y, button)?)
        }
        "drag-window" => {
            let (
                id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                from_x,
                from_y,
                to_x,
                to_y,
                button,
                timeout_ms,
            ) = parse_drag_window_options(&args[1..])?;
            print_json(&workspace::drag_window(
                &id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                from_x,
                from_y,
                to_x,
                to_y,
                button,
                timeout_ms,
            )?)
        }
        "scroll" => {
            let (id, x, y, direction, amount) = parse_scroll_options(&args[1..])?;
            print_json(&workspace::scroll(&id, x, y, direction, amount)?)
        }
        "scroll-window" => {
            let (
                id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                x,
                y,
                direction,
                amount,
                timeout_ms,
            ) = parse_scroll_window_options(&args[1..])?;
            print_json(&workspace::scroll_window(
                &id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                x,
                y,
                direction,
                amount,
                timeout_ms,
            )?)
        }
        "key" => {
            let (id, key) = parse_one_arg_command(&args[1..], "workspace key requires a key")?;
            print_json(&workspace::key(&id, key)?)
        }
        "key-window" => {
            let (id, window_id, title_contains, class_contains, pid, app_id, key, timeout_ms) =
                parse_key_window_options(&args[1..])?;
            print_json(&workspace::key_window(
                &id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                key,
                timeout_ms,
            )?)
        }
        "type" => {
            let (id, text) = parse_text_command(&args[1..])?;
            print_json(&workspace::type_text(&id, text)?)
        }
        "type-window" => {
            let (id, window_id, title_contains, class_contains, pid, app_id, text, timeout_ms) =
                parse_type_window_options(&args[1..])?;
            print_json(&workspace::type_window(
                &id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                text,
                timeout_ms,
            )?)
        }
        "clipboard-set" => {
            let (id, text) = parse_clipboard_set_options(&args[1..])?;
            print_json(&workspace::set_clipboard(&id, text)?)
        }
        "clipboard-get" => {
            let id = parse_id_option(&args[1..])?;
            print_json(&workspace::get_clipboard(&id)?)
        }
        "paste" => {
            let (id, text, key) = parse_paste_options(&args[1..])?;
            print_json(&workspace::paste_text(&id, text, key)?)
        }
        "paste-window" => {
            let (id, window_id, title_contains, class_contains, pid, app_id, text, key, timeout_ms) =
                parse_paste_window_options(&args[1..])?;
            print_json(&workspace::paste_window(
                &id,
                window_id,
                title_contains,
                class_contains,
                pid,
                app_id,
                text,
                key,
                timeout_ms,
            )?)
        }
        "logs" => {
            let (id, app_id, stream, tail_bytes) = parse_logs_options(&args[1..])?;
            print_json(&workspace::read_app_log(&id, app_id, stream, tail_bytes)?)
        }
        "wait-app" => {
            let (id, app_id, timeout_ms, kill_on_timeout) = parse_wait_app_options(&args[1..])?;
            print_json(&workspace::wait_app(
                &id,
                app_id,
                timeout_ms,
                kill_on_timeout,
            )?)
        }
        "events" => {
            let (id, tail, since_sequence) = parse_events_options(&args[1..])?;
            print_json(&workspace::read_events(&id, tail, since_sequence)?)
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
            let (id, timeout_ms) = parse_stop_options(&args[1..])?;
            print_json(&workspace::stop_workspace(&id, timeout_ms)?)
        }
        unknown => {
            bail!(
                "unknown workspace command '{unknown}'. Expected: {}",
                "start, open-profile, list, cleanup, status, ipc-info, launch, run, launch-profile-apps, apps, windows, active-window, observe, wait-window, screenshot, screenshot-window, focus-window, close-window, move-window, resize-window, raise-window, minimize-window, show-window, click, click-window, move-pointer, move-pointer-window, drag, drag-window, scroll, scroll-window, key, key-window, type, type-window, clipboard-set, clipboard-get, paste, paste-window, logs, wait-app, events, setup, kill-app, stop"
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
            "--purpose" => {
                options.purpose = Some(value_after(args, index, "--purpose")?.to_string());
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
            "--purpose" => {
                options.purpose = Some(value_after(args, index, "--purpose")?.to_string());
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
                open_options.setup.wait = true;
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
            "--setup-kill-on-timeout" => {
                open_options.run_setup = true;
                open_options.setup.wait = true;
                open_options.setup.kill_on_timeout = true;
                index += 1;
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

struct ParsedAppsOptions {
    id: String,
    app_id: Option<String>,
    name_contains: Option<String>,
    command_contains: Option<String>,
    profile_id: Option<String>,
    running: Option<bool>,
}

fn parse_apps_options(args: &[String]) -> Result<ParsedAppsOptions> {
    let mut id = workspace::default_workspace_id();
    let mut app_id = None;
    let mut name_contains = None;
    let mut command_contains = None;
    let mut profile_id = None;
    let mut running = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--app" => {
                app_id = Some(value_after(args, index, "--app")?.to_string());
                index += 2;
            }
            "--name" => {
                name_contains = Some(value_after(args, index, "--name")?.to_string());
                index += 2;
            }
            "--command" => {
                command_contains = Some(value_after(args, index, "--command")?.to_string());
                index += 2;
            }
            "--profile" => {
                profile_id = Some(value_after(args, index, "--profile")?.to_string());
                index += 2;
            }
            "--running" => {
                if running == Some(false) {
                    bail!("workspace apps accepts only one of --running or --stopped");
                }
                running = Some(true);
                index += 1;
            }
            "--stopped" => {
                if running == Some(true) {
                    bail!("workspace apps accepts only one of --running or --stopped");
                }
                running = Some(false);
                index += 1;
            }
            flag => bail!("unknown workspace apps option '{flag}'"),
        }
    }
    Ok(ParsedAppsOptions {
        id,
        app_id,
        name_contains,
        command_contains,
        profile_id,
        running,
    })
}

fn parse_windows_options(
    args: &[String],
) -> Result<(
    String,
    bool,
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<String>,
)> {
    let mut id = workspace::default_workspace_id();
    let mut include_hidden = false;
    let mut title_contains = None;
    let mut class_contains = None;
    let mut pid = None;
    let mut app_id = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--all" | "--include-hidden" => {
                include_hidden = true;
                index += 1;
            }
            "--title" => {
                title_contains = Some(value_after(args, index, "--title")?.to_string());
                index += 2;
            }
            "--class" => {
                class_contains = Some(value_after(args, index, "--class")?.to_string());
                index += 2;
            }
            "--pid" => {
                pid = Some(
                    value_after(args, index, "--pid")?
                        .parse()
                        .context("--pid must be a positive integer")?,
                );
                index += 2;
            }
            "--app" => {
                app_id = Some(value_after(args, index, "--app")?.to_string());
                index += 2;
            }
            flag => bail!("unknown workspace windows option '{flag}'"),
        }
    }
    Ok((
        id,
        include_hidden,
        title_contains,
        class_contains,
        pid,
        app_id,
    ))
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
    let mut name = None;
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
            "--name" => {
                name = Some(value_after(args, index, "--name")?.to_string());
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
                    name,
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
                    name,
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

fn parse_run_options(
    args: &[String],
) -> Result<(String, LaunchSpec, Option<u64>, Option<u64>, bool)> {
    let mut id = workspace::default_workspace_id();
    let mut name = None;
    let mut profile_id = None;
    let mut cwd = None;
    let mut cwd_explicit = false;
    let mut user_acknowledged_unenforced_policy = false;
    let mut timeout_ms = None;
    let mut tail_bytes = None;
    let mut kill_on_timeout = false;
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
            "--name" => {
                name = Some(value_after(args, index, "--name")?.to_string());
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
            "--kill-on-timeout" => {
                kill_on_timeout = true;
                index += 1;
            }
            "--" => {
                let command = args[index + 1..].to_vec();
                if command.is_empty() {
                    bail!("workspace run requires a command after --");
                }
                let mut spec = LaunchSpec {
                    command,
                    name,
                    profile_id: None,
                    applied_policy: None,
                    user_acknowledged_unenforced_policy,
                    cwd,
                    env,
                };
                if let Some(profile_id) = &profile_id {
                    profile::apply_profile_to_launch_spec(profile_id, &mut spec, cwd_explicit)?;
                }
                return Ok((id, spec, timeout_ms, tail_bytes, kill_on_timeout));
            }
            _ => {
                let command = args[index..].to_vec();
                if command.is_empty() {
                    bail!("workspace run requires a command");
                }
                let mut spec = LaunchSpec {
                    command,
                    name,
                    profile_id: None,
                    applied_policy: None,
                    user_acknowledged_unenforced_policy,
                    cwd,
                    env,
                };
                if let Some(profile_id) = &profile_id {
                    profile::apply_profile_to_launch_spec(profile_id, &mut spec, cwd_explicit)?;
                }
                return Ok((id, spec, timeout_ms, tail_bytes, kill_on_timeout));
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

fn parse_observe_options(args: &[String]) -> Result<(String, bool, bool, Option<PathBuf>)> {
    let mut id = workspace::default_workspace_id();
    let mut screenshot = false;
    let mut include_hidden = false;
    let mut output_path = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--screenshot" => {
                screenshot = true;
                index += 1;
            }
            "--all-windows" | "--include-hidden" => {
                include_hidden = true;
                index += 1;
            }
            "--output" => {
                output_path = Some(PathBuf::from(value_after(args, index, "--output")?));
                index += 2;
            }
            flag => bail!("unknown workspace observe option '{flag}'"),
        }
    }
    if output_path.is_some() && !screenshot {
        bail!("workspace observe --output requires --screenshot");
    }
    Ok((id, screenshot, include_hidden, output_path))
}

type ScreenshotWindowOptions = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<String>,
    Option<PathBuf>,
    Option<u64>,
);

fn parse_screenshot_window_options(args: &[String]) -> Result<ScreenshotWindowOptions> {
    let mut id = workspace::default_workspace_id();
    let mut window_id = None;
    let mut title_contains = None;
    let mut class_contains = None;
    let mut pid = None;
    let mut app_id = None;
    let mut output_path = None;
    let mut timeout_ms = None;
    let mut positional = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--window" => {
                window_id = Some(value_after(args, index, "--window")?.to_string());
                index += 2;
            }
            "--title" => {
                title_contains = Some(value_after(args, index, "--title")?.to_string());
                index += 2;
            }
            "--class" => {
                class_contains = Some(value_after(args, index, "--class")?.to_string());
                index += 2;
            }
            "--pid" => {
                pid = Some(
                    value_after(args, index, "--pid")?
                        .parse()
                        .context("--pid must be a positive integer")?,
                );
                index += 2;
            }
            "--app" => {
                app_id = Some(value_after(args, index, "--app")?.to_string());
                index += 2;
            }
            "--output" => {
                output_path = Some(PathBuf::from(value_after(args, index, "--output")?));
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
            value if value.starts_with("--") => {
                bail!("unknown workspace screenshot-window option '{value}'")
            }
            value => {
                positional.push(value.to_string());
                index += 1;
            }
        }
    }

    if positional.len() > 1 {
        bail!("workspace screenshot-window accepts at most one positional window id");
    }
    if let Some(positional_window_id) = positional.into_iter().next() {
        if window_id.is_some() {
            bail!("workspace screenshot-window accepts only one window id");
        }
        window_id = Some(positional_window_id);
    }
    let has_match_filter =
        title_contains.is_some() || class_contains.is_some() || pid.is_some() || app_id.is_some();
    if window_id.is_some() && has_match_filter {
        bail!("workspace screenshot-window accepts either a window id or match filters, not both");
    }
    if window_id.is_none() && !has_match_filter {
        bail!(
            "workspace screenshot-window requires a window id or --title, --class, --pid, or --app"
        );
    }
    if window_id.is_some() && timeout_ms.is_some() {
        bail!("workspace screenshot-window accepts --timeout-ms only with match filters");
    }

    Ok((
        id,
        window_id,
        title_contains,
        class_contains,
        pid,
        app_id,
        output_path,
        timeout_ms,
    ))
}

fn parse_wait_window_options(
    args: &[String],
) -> Result<(
    String,
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<String>,
    Option<u64>,
)> {
    let mut id = workspace::default_workspace_id();
    let mut title_contains = None;
    let mut class_contains = None;
    let mut pid = None;
    let mut app_id = None;
    let mut timeout_ms = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--title" => {
                title_contains = Some(value_after(args, index, "--title")?.to_string());
                index += 2;
            }
            "--class" => {
                class_contains = Some(value_after(args, index, "--class")?.to_string());
                index += 2;
            }
            "--pid" => {
                pid = Some(
                    value_after(args, index, "--pid")?
                        .parse()
                        .context("--pid must be a positive integer")?,
                );
                index += 2;
            }
            "--app" => {
                app_id = Some(value_after(args, index, "--app")?.to_string());
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
            flag => bail!("unknown workspace wait-window option '{flag}'"),
        }
    }
    Ok((id, title_contains, class_contains, pid, app_id, timeout_ms))
}

enum FocusWindowTarget {
    WindowId(String),
    Match {
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        timeout_ms: Option<u64>,
    },
}

fn parse_focus_window_options(args: &[String]) -> Result<(String, FocusWindowTarget)> {
    let mut id = workspace::default_workspace_id();
    let mut window_id = None;
    let mut title_contains = None;
    let mut class_contains = None;
    let mut pid = None;
    let mut app_id = None;
    let mut timeout_ms = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--title" => {
                title_contains = Some(value_after(args, index, "--title")?.to_string());
                index += 2;
            }
            "--class" => {
                class_contains = Some(value_after(args, index, "--class")?.to_string());
                index += 2;
            }
            "--pid" => {
                pid = Some(
                    value_after(args, index, "--pid")?
                        .parse()
                        .context("--pid must be a positive integer")?,
                );
                index += 2;
            }
            "--app" => {
                app_id = Some(value_after(args, index, "--app")?.to_string());
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
            value if value.starts_with("--") => {
                bail!("unknown workspace focus-window option '{value}'")
            }
            value => {
                if window_id.is_some() {
                    bail!("workspace focus-window accepts only one window id");
                }
                window_id = Some(value.to_string());
                index += 1;
            }
        }
    }

    let has_match_filter =
        title_contains.is_some() || class_contains.is_some() || pid.is_some() || app_id.is_some();
    if let Some(window_id) = window_id {
        if has_match_filter || timeout_ms.is_some() {
            bail!("workspace focus-window accepts either a window id or match options, not both");
        }
        return Ok((id, FocusWindowTarget::WindowId(window_id)));
    }
    if !has_match_filter {
        bail!("workspace focus-window requires a window id or --title, --class, --pid, or --app");
    }
    Ok((
        id,
        FocusWindowTarget::Match {
            title_contains,
            class_contains,
            pid,
            app_id,
            timeout_ms,
        },
    ))
}

enum CloseWindowTarget {
    WindowId(String),
    Match {
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        timeout_ms: Option<u64>,
    },
}

fn parse_close_window_options(args: &[String]) -> Result<(String, CloseWindowTarget)> {
    let mut id = workspace::default_workspace_id();
    let mut window_id = None;
    let mut title_contains = None;
    let mut class_contains = None;
    let mut pid = None;
    let mut app_id = None;
    let mut timeout_ms = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--title" => {
                title_contains = Some(value_after(args, index, "--title")?.to_string());
                index += 2;
            }
            "--class" => {
                class_contains = Some(value_after(args, index, "--class")?.to_string());
                index += 2;
            }
            "--pid" => {
                pid = Some(
                    value_after(args, index, "--pid")?
                        .parse()
                        .context("--pid must be a positive integer")?,
                );
                index += 2;
            }
            "--app" => {
                app_id = Some(value_after(args, index, "--app")?.to_string());
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
            value if value.starts_with("--") => {
                bail!("unknown workspace close-window option '{value}'")
            }
            value => {
                if window_id.is_some() {
                    bail!("workspace close-window accepts only one window id");
                }
                window_id = Some(value.to_string());
                index += 1;
            }
        }
    }

    let has_match_filter =
        title_contains.is_some() || class_contains.is_some() || pid.is_some() || app_id.is_some();
    if let Some(window_id) = window_id {
        if has_match_filter || timeout_ms.is_some() {
            bail!("workspace close-window accepts either a window id or match options, not both");
        }
        return Ok((id, CloseWindowTarget::WindowId(window_id)));
    }
    if !has_match_filter {
        bail!("workspace close-window requires a window id or --title, --class, --pid, or --app");
    }
    Ok((
        id,
        CloseWindowTarget::Match {
            title_contains,
            class_contains,
            pid,
            app_id,
            timeout_ms,
        },
    ))
}

type MoveWindowOptions = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<String>,
    i32,
    i32,
    Option<u64>,
);

fn parse_move_window_options(args: &[String]) -> Result<MoveWindowOptions> {
    let (id, title_contains, class_contains, pid, app_id, timeout_ms, values) =
        parse_window_target_values(args, "workspace move-window")?;
    let has_match_filter =
        title_contains.is_some() || class_contains.is_some() || pid.is_some() || app_id.is_some();
    let (window_id, x_value, y_value) = if has_match_filter {
        if values.len() != 2 {
            bail!("workspace move-window with match filters requires X and Y coordinates");
        }
        (None, &values[0], &values[1])
    } else {
        if values.len() != 3 {
            bail!("workspace move-window requires WINDOW_ID X Y or match filters with X Y");
        }
        if timeout_ms.is_some() {
            bail!("workspace move-window accepts --timeout-ms only with match filters");
        }
        (Some(values[0].clone()), &values[1], &values[2])
    };
    let x = x_value
        .parse()
        .context("move-window X must be an integer")?;
    let y = y_value
        .parse()
        .context("move-window Y must be an integer")?;
    Ok((
        id,
        window_id,
        title_contains,
        class_contains,
        pid,
        app_id,
        x,
        y,
        timeout_ms,
    ))
}

type ResizeWindowOptions = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<String>,
    u32,
    u32,
    Option<u64>,
);

fn parse_resize_window_options(args: &[String]) -> Result<ResizeWindowOptions> {
    let (id, title_contains, class_contains, pid, app_id, timeout_ms, values) =
        parse_window_target_values(args, "workspace resize-window")?;
    let has_match_filter =
        title_contains.is_some() || class_contains.is_some() || pid.is_some() || app_id.is_some();
    let (window_id, width_value, height_value) = if has_match_filter {
        if values.len() != 2 {
            bail!("workspace resize-window with match filters requires WIDTH and HEIGHT");
        }
        (None, &values[0], &values[1])
    } else {
        if values.len() != 3 {
            bail!("workspace resize-window requires WINDOW_ID WIDTH HEIGHT or match filters with WIDTH HEIGHT");
        }
        if timeout_ms.is_some() {
            bail!("workspace resize-window accepts --timeout-ms only with match filters");
        }
        (Some(values[0].clone()), &values[1], &values[2])
    };
    let width = width_value
        .parse()
        .context("resize-window WIDTH must be a positive integer")?;
    let height = height_value
        .parse()
        .context("resize-window HEIGHT must be a positive integer")?;
    Ok((
        id,
        window_id,
        title_contains,
        class_contains,
        pid,
        app_id,
        width,
        height,
        timeout_ms,
    ))
}

type TargetedWindowActionOptions = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<String>,
    Option<u64>,
);

fn parse_targeted_window_action_options(
    args: &[String],
    command_name: &str,
) -> Result<TargetedWindowActionOptions> {
    let (id, title_contains, class_contains, pid, app_id, timeout_ms, values) =
        parse_window_target_values(args, command_name)?;
    let has_match_filter =
        title_contains.is_some() || class_contains.is_some() || pid.is_some() || app_id.is_some();
    let window_id = if has_match_filter {
        if !values.is_empty() {
            bail!("{command_name} with match filters does not accept a window id");
        }
        None
    } else {
        if values.len() != 1 {
            bail!("{command_name} requires WINDOW_ID or match filters");
        }
        if timeout_ms.is_some() {
            bail!("{command_name} accepts --timeout-ms only with match filters");
        }
        Some(values[0].clone())
    };
    Ok((
        id,
        window_id,
        title_contains,
        class_contains,
        pid,
        app_id,
        timeout_ms,
    ))
}

fn parse_click_options(args: &[String]) -> Result<(String, i32, i32, Option<u8>, Option<u8>)> {
    let mut id = workspace::default_workspace_id();
    let mut button = None;
    let mut count = None;
    let mut values = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--button" => {
                button = Some(
                    value_after(args, index, "--button")?
                        .parse()
                        .context("--button must be an integer between 1 and 5")?,
                );
                index += 2;
            }
            "--count" => {
                count = Some(
                    value_after(args, index, "--count")?
                        .parse()
                        .context("--count must be an integer between 1 and 20")?,
                );
                index += 2;
            }
            value if value.starts_with("--") => bail!("unknown workspace click option '{value}'"),
            value => {
                values.push(value.to_string());
                index += 1;
            }
        }
    }
    if values.len() != 2 {
        bail!("workspace click requires X and Y coordinates");
    }
    let x = values[0].parse().context("click X must be an integer")?;
    let y = values[1].parse().context("click Y must be an integer")?;
    Ok((id, x, y, button, count))
}

type ClickWindowOptions = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<String>,
    i32,
    i32,
    Option<u8>,
    Option<u8>,
    Option<u64>,
);

fn parse_click_window_options(args: &[String]) -> Result<ClickWindowOptions> {
    let mut id = workspace::default_workspace_id();
    let mut title_contains = None;
    let mut class_contains = None;
    let mut pid = None;
    let mut app_id = None;
    let mut timeout_ms = None;
    let mut button = None;
    let mut count = None;
    let mut values = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--title" => {
                title_contains = Some(value_after(args, index, "--title")?.to_string());
                index += 2;
            }
            "--class" => {
                class_contains = Some(value_after(args, index, "--class")?.to_string());
                index += 2;
            }
            "--pid" => {
                pid = Some(
                    value_after(args, index, "--pid")?
                        .parse()
                        .context("--pid must be a positive integer")?,
                );
                index += 2;
            }
            "--app" => {
                app_id = Some(value_after(args, index, "--app")?.to_string());
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
            "--button" => {
                button = Some(
                    value_after(args, index, "--button")?
                        .parse()
                        .context("--button must be an integer between 1 and 5")?,
                );
                index += 2;
            }
            "--count" => {
                count = Some(
                    value_after(args, index, "--count")?
                        .parse()
                        .context("--count must be an integer between 1 and 20")?,
                );
                index += 2;
            }
            value if value.starts_with("--") => {
                bail!("unknown workspace click-window option '{value}'")
            }
            value => {
                values.push(value.to_string());
                index += 1;
            }
        }
    }

    let has_match_filter =
        title_contains.is_some() || class_contains.is_some() || pid.is_some() || app_id.is_some();
    let (window_id, x_value, y_value) = if has_match_filter {
        if values.len() != 2 {
            bail!("workspace click-window with match filters requires X and Y coordinates");
        }
        (None, &values[0], &values[1])
    } else {
        if values.len() != 3 {
            bail!("workspace click-window requires WINDOW_ID X Y or match filters with X Y");
        }
        if timeout_ms.is_some() {
            bail!("workspace click-window accepts --timeout-ms only with match filters");
        }
        (Some(values[0].clone()), &values[1], &values[2])
    };
    let x = x_value
        .parse()
        .context("click-window X must be an integer")?;
    let y = y_value
        .parse()
        .context("click-window Y must be an integer")?;
    Ok((
        id,
        window_id,
        title_contains,
        class_contains,
        pid,
        app_id,
        x,
        y,
        button,
        count,
        timeout_ms,
    ))
}

fn parse_move_pointer_options(args: &[String]) -> Result<(String, i32, i32)> {
    let mut id = workspace::default_workspace_id();
    let mut values = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            value if value.starts_with("--") => {
                bail!("unknown workspace move-pointer option '{value}'")
            }
            value => {
                values.push(value.to_string());
                index += 1;
            }
        }
    }
    if values.len() != 2 {
        bail!("workspace move-pointer requires X and Y coordinates");
    }
    let x = values[0]
        .parse()
        .context("move-pointer X must be an integer")?;
    let y = values[1]
        .parse()
        .context("move-pointer Y must be an integer")?;
    Ok((id, x, y))
}

type MovePointerWindowOptions = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<String>,
    i32,
    i32,
    Option<u64>,
);

fn parse_move_pointer_window_options(args: &[String]) -> Result<MovePointerWindowOptions> {
    let mut id = workspace::default_workspace_id();
    let mut title_contains = None;
    let mut class_contains = None;
    let mut pid = None;
    let mut app_id = None;
    let mut timeout_ms = None;
    let mut values = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--title" => {
                title_contains = Some(value_after(args, index, "--title")?.to_string());
                index += 2;
            }
            "--class" => {
                class_contains = Some(value_after(args, index, "--class")?.to_string());
                index += 2;
            }
            "--pid" => {
                pid = Some(
                    value_after(args, index, "--pid")?
                        .parse()
                        .context("--pid must be a positive integer")?,
                );
                index += 2;
            }
            "--app" => {
                app_id = Some(value_after(args, index, "--app")?.to_string());
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
            value if value.starts_with("--") => {
                bail!("unknown workspace move-pointer-window option '{value}'")
            }
            value => {
                values.push(value.to_string());
                index += 1;
            }
        }
    }

    let has_match_filter =
        title_contains.is_some() || class_contains.is_some() || pid.is_some() || app_id.is_some();
    let (window_id, x_value, y_value) = if has_match_filter {
        if values.len() != 2 {
            bail!("workspace move-pointer-window with match filters requires X and Y coordinates");
        }
        (None, &values[0], &values[1])
    } else {
        if values.len() != 3 {
            bail!("workspace move-pointer-window requires WINDOW_ID X Y or match filters with X Y");
        }
        if timeout_ms.is_some() {
            bail!("workspace move-pointer-window accepts --timeout-ms only with match filters");
        }
        (Some(values[0].clone()), &values[1], &values[2])
    };
    let x = x_value
        .parse()
        .context("move-pointer-window X must be an integer")?;
    let y = y_value
        .parse()
        .context("move-pointer-window Y must be an integer")?;
    Ok((
        id,
        window_id,
        title_contains,
        class_contains,
        pid,
        app_id,
        x,
        y,
        timeout_ms,
    ))
}

fn parse_drag_options(args: &[String]) -> Result<(String, i32, i32, i32, i32, Option<u8>)> {
    let mut id = workspace::default_workspace_id();
    let mut button = None;
    let mut values = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--button" => {
                button = Some(
                    value_after(args, index, "--button")?
                        .parse()
                        .context("--button must be an integer between 1 and 5")?,
                );
                index += 2;
            }
            value if value.starts_with("--") => bail!("unknown workspace drag option '{value}'"),
            value => {
                values.push(value.to_string());
                index += 1;
            }
        }
    }
    if values.len() != 4 {
        bail!("workspace drag requires FROM_X FROM_Y TO_X TO_Y coordinates");
    }
    let from_x = values[0]
        .parse()
        .context("drag FROM_X must be an integer")?;
    let from_y = values[1]
        .parse()
        .context("drag FROM_Y must be an integer")?;
    let to_x = values[2].parse().context("drag TO_X must be an integer")?;
    let to_y = values[3].parse().context("drag TO_Y must be an integer")?;
    Ok((id, from_x, from_y, to_x, to_y, button))
}

type DragWindowOptions = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<String>,
    i32,
    i32,
    i32,
    i32,
    Option<u8>,
    Option<u64>,
);

fn parse_drag_window_options(args: &[String]) -> Result<DragWindowOptions> {
    let mut id = workspace::default_workspace_id();
    let mut title_contains = None;
    let mut class_contains = None;
    let mut pid = None;
    let mut app_id = None;
    let mut timeout_ms = None;
    let mut button = None;
    let mut values = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--title" => {
                title_contains = Some(value_after(args, index, "--title")?.to_string());
                index += 2;
            }
            "--class" => {
                class_contains = Some(value_after(args, index, "--class")?.to_string());
                index += 2;
            }
            "--pid" => {
                pid = Some(
                    value_after(args, index, "--pid")?
                        .parse()
                        .context("--pid must be a positive integer")?,
                );
                index += 2;
            }
            "--app" => {
                app_id = Some(value_after(args, index, "--app")?.to_string());
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
            "--button" => {
                button = Some(
                    value_after(args, index, "--button")?
                        .parse()
                        .context("--button must be an integer between 1 and 5")?,
                );
                index += 2;
            }
            value if value.starts_with("--") => {
                bail!("unknown workspace drag-window option '{value}'")
            }
            value => {
                values.push(value.to_string());
                index += 1;
            }
        }
    }

    let has_match_filter =
        title_contains.is_some() || class_contains.is_some() || pid.is_some() || app_id.is_some();
    let (window_id, coordinate_values) = if has_match_filter {
        if values.len() != 4 {
            bail!(
                "workspace drag-window with match filters requires FROM_X FROM_Y TO_X TO_Y coordinates"
            );
        }
        (None, values.as_slice())
    } else {
        if values.len() != 5 {
            bail!("workspace drag-window requires WINDOW_ID FROM_X FROM_Y TO_X TO_Y or match filters with FROM_X FROM_Y TO_X TO_Y");
        }
        if timeout_ms.is_some() {
            bail!("workspace drag-window accepts --timeout-ms only with match filters");
        }
        (Some(values[0].clone()), &values[1..])
    };
    let from_x = coordinate_values[0]
        .parse()
        .context("drag-window FROM_X must be an integer")?;
    let from_y = coordinate_values[1]
        .parse()
        .context("drag-window FROM_Y must be an integer")?;
    let to_x = coordinate_values[2]
        .parse()
        .context("drag-window TO_X must be an integer")?;
    let to_y = coordinate_values[3]
        .parse()
        .context("drag-window TO_Y must be an integer")?;
    Ok((
        id,
        window_id,
        title_contains,
        class_contains,
        pid,
        app_id,
        from_x,
        from_y,
        to_x,
        to_y,
        button,
        timeout_ms,
    ))
}

fn parse_scroll_options(
    args: &[String],
) -> Result<(String, i32, i32, workspace::ScrollDirection, Option<u8>)> {
    let mut id = workspace::default_workspace_id();
    let mut amount = None;
    let mut values = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--amount" => {
                amount = Some(
                    value_after(args, index, "--amount")?
                        .parse()
                        .context("--amount must be an integer between 1 and 100")?,
                );
                index += 2;
            }
            value if value.starts_with("--") => bail!("unknown workspace scroll option '{value}'"),
            value => {
                values.push(value.to_string());
                index += 1;
            }
        }
    }
    if values.len() != 3 {
        bail!("workspace scroll requires X Y DIRECTION");
    }
    let x = values[0].parse().context("scroll X must be an integer")?;
    let y = values[1].parse().context("scroll Y must be an integer")?;
    let direction = values[2]
        .parse()
        .context("scroll DIRECTION must be up, down, left, or right")?;
    Ok((id, x, y, direction, amount))
}

type ScrollWindowOptions = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<String>,
    i32,
    i32,
    workspace::ScrollDirection,
    Option<u8>,
    Option<u64>,
);

fn parse_scroll_window_options(args: &[String]) -> Result<ScrollWindowOptions> {
    let mut id = workspace::default_workspace_id();
    let mut title_contains = None;
    let mut class_contains = None;
    let mut pid = None;
    let mut app_id = None;
    let mut timeout_ms = None;
    let mut amount = None;
    let mut values = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--title" => {
                title_contains = Some(value_after(args, index, "--title")?.to_string());
                index += 2;
            }
            "--class" => {
                class_contains = Some(value_after(args, index, "--class")?.to_string());
                index += 2;
            }
            "--pid" => {
                pid = Some(
                    value_after(args, index, "--pid")?
                        .parse()
                        .context("--pid must be a positive integer")?,
                );
                index += 2;
            }
            "--app" => {
                app_id = Some(value_after(args, index, "--app")?.to_string());
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
            "--amount" => {
                amount = Some(
                    value_after(args, index, "--amount")?
                        .parse()
                        .context("--amount must be an integer between 1 and 100")?,
                );
                index += 2;
            }
            value if value.starts_with("--") => {
                bail!("unknown workspace scroll-window option '{value}'")
            }
            value => {
                values.push(value.to_string());
                index += 1;
            }
        }
    }

    let has_match_filter =
        title_contains.is_some() || class_contains.is_some() || pid.is_some() || app_id.is_some();
    let (window_id, x_value, y_value, direction_value) = if has_match_filter {
        if values.len() != 3 {
            bail!("workspace scroll-window with match filters requires X Y DIRECTION");
        }
        (None, &values[0], &values[1], &values[2])
    } else {
        if values.len() != 4 {
            bail!("workspace scroll-window requires WINDOW_ID X Y DIRECTION or match filters with X Y DIRECTION");
        }
        if timeout_ms.is_some() {
            bail!("workspace scroll-window accepts --timeout-ms only with match filters");
        }
        (Some(values[0].clone()), &values[1], &values[2], &values[3])
    };
    let x = x_value
        .parse()
        .context("scroll-window X must be an integer")?;
    let y = y_value
        .parse()
        .context("scroll-window Y must be an integer")?;
    let direction = direction_value
        .parse()
        .context("scroll-window DIRECTION must be up, down, left, or right")?;
    Ok((
        id,
        window_id,
        title_contains,
        class_contains,
        pid,
        app_id,
        x,
        y,
        direction,
        amount,
        timeout_ms,
    ))
}

type WindowTargetValues = (
    String,
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<String>,
    Option<u64>,
    Vec<String>,
);

fn parse_window_target_values(args: &[String], command_name: &str) -> Result<WindowTargetValues> {
    let mut id = workspace::default_workspace_id();
    let mut title_contains = None;
    let mut class_contains = None;
    let mut pid = None;
    let mut app_id = None;
    let mut timeout_ms = None;
    let mut values = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--title" => {
                title_contains = Some(value_after(args, index, "--title")?.to_string());
                index += 2;
            }
            "--class" => {
                class_contains = Some(value_after(args, index, "--class")?.to_string());
                index += 2;
            }
            "--pid" => {
                pid = Some(
                    value_after(args, index, "--pid")?
                        .parse()
                        .context("--pid must be a positive integer")?,
                );
                index += 2;
            }
            "--app" => {
                app_id = Some(value_after(args, index, "--app")?.to_string());
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
            value if value.starts_with("--") => bail!("unknown {command_name} option '{value}'"),
            value => {
                values.push(value.to_string());
                index += 1;
            }
        }
    }
    Ok((
        id,
        title_contains,
        class_contains,
        pid,
        app_id,
        timeout_ms,
        values,
    ))
}

type KeyWindowOptions = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<String>,
    String,
    Option<u64>,
);

fn parse_key_window_options(args: &[String]) -> Result<KeyWindowOptions> {
    let (id, title_contains, class_contains, pid, app_id, timeout_ms, values) =
        parse_window_target_values(args, "workspace key-window")?;
    let has_match_filter =
        title_contains.is_some() || class_contains.is_some() || pid.is_some() || app_id.is_some();
    let (window_id, key) = if has_match_filter {
        if values.len() != 1 {
            bail!("workspace key-window with match filters requires a key");
        }
        (None, values[0].clone())
    } else {
        if values.len() != 2 {
            bail!("workspace key-window requires WINDOW_ID KEY or match filters with KEY");
        }
        if timeout_ms.is_some() {
            bail!("workspace key-window accepts --timeout-ms only with match filters");
        }
        (Some(values[0].clone()), values[1].clone())
    };
    Ok((
        id,
        window_id,
        title_contains,
        class_contains,
        pid,
        app_id,
        key,
        timeout_ms,
    ))
}

type TypeWindowOptions = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<String>,
    String,
    Option<u64>,
);

fn parse_type_window_options(args: &[String]) -> Result<TypeWindowOptions> {
    let (id, title_contains, class_contains, pid, app_id, timeout_ms, values) =
        parse_window_target_values(args, "workspace type-window")?;
    let has_match_filter =
        title_contains.is_some() || class_contains.is_some() || pid.is_some() || app_id.is_some();
    let (window_id, text_values) = if has_match_filter {
        if values.is_empty() {
            bail!("workspace type-window with match filters requires text");
        }
        (None, values)
    } else {
        if values.len() < 2 {
            bail!("workspace type-window requires WINDOW_ID TEXT or match filters with TEXT");
        }
        if timeout_ms.is_some() {
            bail!("workspace type-window accepts --timeout-ms only with match filters");
        }
        (Some(values[0].clone()), values[1..].to_vec())
    };
    Ok((
        id,
        window_id,
        title_contains,
        class_contains,
        pid,
        app_id,
        text_values.join(" "),
        timeout_ms,
    ))
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

fn parse_clipboard_set_options(args: &[String]) -> Result<(String, String)> {
    let (id, values) = parse_id_and_args(args)?;
    if values.is_empty() {
        bail!("workspace clipboard-set requires text");
    }
    Ok((id, values.join(" ")))
}

fn parse_paste_options(args: &[String]) -> Result<(String, String, Option<String>)> {
    let mut id = workspace::default_workspace_id();
    let mut key = None;
    let mut values = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--key" => {
                key = Some(value_after(args, index, "--key")?.to_string());
                index += 2;
            }
            value if value.starts_with("--") => bail!("unknown workspace paste option '{value}'"),
            value => {
                values.push(value.to_string());
                index += 1;
            }
        }
    }
    if values.is_empty() {
        bail!("workspace paste requires text");
    }
    Ok((id, values.join(" "), key))
}

type PasteWindowOptions = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<String>,
    String,
    Option<String>,
    Option<u64>,
);

fn parse_paste_window_options(args: &[String]) -> Result<PasteWindowOptions> {
    let mut id = workspace::default_workspace_id();
    let mut title_contains = None;
    let mut class_contains = None;
    let mut pid = None;
    let mut app_id = None;
    let mut timeout_ms = None;
    let mut key = None;
    let mut values = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--title" => {
                title_contains = Some(value_after(args, index, "--title")?.to_string());
                index += 2;
            }
            "--class" => {
                class_contains = Some(value_after(args, index, "--class")?.to_string());
                index += 2;
            }
            "--pid" => {
                pid = Some(
                    value_after(args, index, "--pid")?
                        .parse()
                        .context("--pid must be a positive integer")?,
                );
                index += 2;
            }
            "--app" => {
                app_id = Some(value_after(args, index, "--app")?.to_string());
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
            "--key" => {
                key = Some(value_after(args, index, "--key")?.to_string());
                index += 2;
            }
            value if value.starts_with("--") => {
                bail!("unknown workspace paste-window option '{value}'")
            }
            value => {
                values.push(value.to_string());
                index += 1;
            }
        }
    }

    let has_match_filter =
        title_contains.is_some() || class_contains.is_some() || pid.is_some() || app_id.is_some();
    let (window_id, text_values) = if has_match_filter {
        if values.is_empty() {
            bail!("workspace paste-window with match filters requires text");
        }
        (None, values)
    } else {
        if values.len() < 2 {
            bail!("workspace paste-window requires WINDOW_ID TEXT or match filters with TEXT");
        }
        if timeout_ms.is_some() {
            bail!("workspace paste-window accepts --timeout-ms only with match filters");
        }
        (Some(values[0].clone()), values[1..].to_vec())
    };
    Ok((
        id,
        window_id,
        title_contains,
        class_contains,
        pid,
        app_id,
        text_values.join(" "),
        key,
        timeout_ms,
    ))
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

fn parse_wait_app_options(args: &[String]) -> Result<(String, String, Option<u64>, bool)> {
    let mut id = workspace::default_workspace_id();
    let mut timeout_ms = None;
    let mut kill_on_timeout = false;
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
            "--kill-on-timeout" => {
                kill_on_timeout = true;
                index += 1;
            }
            "--" => {
                let app_id = args
                    .get(index + 1)
                    .context("workspace wait-app requires an app id")?
                    .to_string();
                return Ok((id, app_id, timeout_ms, kill_on_timeout));
            }
            _ => {
                let app_id = args[index].clone();
                return Ok((id, app_id, timeout_ms, kill_on_timeout));
            }
        }
    }
    bail!("workspace wait-app requires an app id")
}

fn parse_events_options(args: &[String]) -> Result<(String, Option<usize>, Option<u64>)> {
    let mut id = workspace::default_workspace_id();
    let mut tail = None;
    let mut since_sequence = None;
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
            "--since" => {
                since_sequence = Some(
                    value_after(args, index, "--since")?
                        .parse()
                        .context("--since must be a non-negative integer")?,
                );
                index += 2;
            }
            flag => bail!("unknown workspace events option '{flag}'"),
        }
    }
    Ok((id, tail, since_sequence))
}

fn parse_stop_options(args: &[String]) -> Result<(String, Option<u64>)> {
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
            flag => bail!("unknown workspace stop option '{flag}'"),
        }
    }
    Ok((id, timeout_ms))
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
            "--kill-on-timeout" => {
                options.wait = true;
                options.kill_on_timeout = true;
                index += 1;
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
    let mut purpose = None;
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
            "--purpose" => {
                purpose = Some(value_after(&args, index, "--purpose")?.to_string());
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
        purpose,
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
        "{}",
        r#"agent-workspace-linux

Usage:
  agent-workspace-linux doctor
  agent-workspace-linux mcp
  agent-workspace-linux profile path|list|get|check|template|put|delete
  agent-workspace-linux profile template project-dev [--id ID] [--host-path PATH]
  agent-workspace-linux workspace start --ack-hidden-workspace [--ack-unenforced-policy] [--foreground] [--profile PROFILE] [--id ID] [--purpose TEXT] [--width PX] [--height PX]
  agent-workspace-linux workspace open-profile --ack-hidden-workspace [--ack-unenforced-policy] --profile PROFILE [--setup] [--setup-timeout-ms N] [--setup-kill-on-timeout] [--id ID] [--purpose TEXT] [--width PX] [--height PX]
  agent-workspace-linux workspace list
  agent-workspace-linux workspace cleanup [--id ID]
  agent-workspace-linux workspace status [--id ID]
  agent-workspace-linux workspace ipc-info [--id ID]
  agent-workspace-linux workspace launch [--id ID] [--name NAME] [--profile PROFILE] [--ack-unenforced-policy] [--cwd DIR] [--env NAME=VALUE] -- COMMAND [ARGS...]
  agent-workspace-linux workspace run [--id ID] [--name NAME] [--profile PROFILE] [--timeout-ms N] [--tail-bytes N] [--kill-on-timeout] -- COMMAND [ARGS...]
  agent-workspace-linux workspace launch-profile-apps [--id ID] --profile PROFILE [--ack-unenforced-policy]
  agent-workspace-linux workspace apps [--id ID] [--app APP_ID_OR_PID_OR_NAME] [--name TEXT] [--command TEXT] [--profile PROFILE] [--running|--stopped]
  agent-workspace-linux workspace windows [--id ID] [--all] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME]
  agent-workspace-linux workspace active-window [--id ID]
  agent-workspace-linux workspace observe [--id ID] [--all-windows] [--screenshot] [--output PATH]
  agent-workspace-linux workspace wait-window [--id ID] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME] [--timeout-ms N]
  agent-workspace-linux workspace screenshot [--id ID] [--output PATH]
  agent-workspace-linux workspace screenshot-window [--id ID] [--window WINDOW_ID] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME] [--output PATH] [--timeout-ms N]
  agent-workspace-linux workspace focus-window [--id ID] WINDOW_ID
  agent-workspace-linux workspace focus-window [--id ID] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME] [--timeout-ms N]
  agent-workspace-linux workspace close-window [--id ID] WINDOW_ID
  agent-workspace-linux workspace close-window [--id ID] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME] [--timeout-ms N]
  agent-workspace-linux workspace move-window [--id ID] WINDOW_ID X Y
  agent-workspace-linux workspace move-window [--id ID] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME] [--timeout-ms N] X Y
  agent-workspace-linux workspace resize-window [--id ID] WINDOW_ID WIDTH HEIGHT
  agent-workspace-linux workspace resize-window [--id ID] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME] [--timeout-ms N] WIDTH HEIGHT
  agent-workspace-linux workspace raise-window [--id ID] WINDOW_ID
  agent-workspace-linux workspace raise-window [--id ID] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME] [--timeout-ms N]
  agent-workspace-linux workspace minimize-window [--id ID] WINDOW_ID
  agent-workspace-linux workspace minimize-window [--id ID] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME] [--timeout-ms N]
  agent-workspace-linux workspace show-window [--id ID] WINDOW_ID
  agent-workspace-linux workspace show-window [--id ID] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME] [--timeout-ms N]
  agent-workspace-linux workspace click [--id ID] [--button N] [--count N] X Y
  agent-workspace-linux workspace click-window [--id ID] WINDOW_ID X Y
  agent-workspace-linux workspace click-window [--id ID] [--button N] [--count N] WINDOW_ID X Y
  agent-workspace-linux workspace click-window [--id ID] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME] [--button N] [--count N] [--timeout-ms N] X Y
  agent-workspace-linux workspace move-pointer [--id ID] X Y
  agent-workspace-linux workspace move-pointer-window [--id ID] WINDOW_ID X Y
  agent-workspace-linux workspace move-pointer-window [--id ID] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME] [--timeout-ms N] X Y
  agent-workspace-linux workspace drag [--id ID] [--button N] FROM_X FROM_Y TO_X TO_Y
  agent-workspace-linux workspace drag-window [--id ID] [--button N] WINDOW_ID FROM_X FROM_Y TO_X TO_Y
  agent-workspace-linux workspace drag-window [--id ID] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME] [--button N] [--timeout-ms N] FROM_X FROM_Y TO_X TO_Y
  agent-workspace-linux workspace scroll [--id ID] [--amount N] X Y up|down|left|right
  agent-workspace-linux workspace scroll-window [--id ID] [--amount N] WINDOW_ID X Y up|down|left|right
  agent-workspace-linux workspace scroll-window [--id ID] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME] [--amount N] [--timeout-ms N] X Y up|down|left|right
  agent-workspace-linux workspace key [--id ID] KEY
  agent-workspace-linux workspace key-window [--id ID] WINDOW_ID KEY
  agent-workspace-linux workspace key-window [--id ID] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME] [--timeout-ms N] KEY
  agent-workspace-linux workspace type [--id ID] TEXT
  agent-workspace-linux workspace type-window [--id ID] WINDOW_ID TEXT
  agent-workspace-linux workspace type-window [--id ID] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME] [--timeout-ms N] TEXT
  agent-workspace-linux workspace clipboard-set [--id ID] TEXT
  agent-workspace-linux workspace clipboard-get [--id ID]
  agent-workspace-linux workspace paste [--id ID] [--key KEY] TEXT
  agent-workspace-linux workspace paste-window [--id ID] [--key KEY] WINDOW_ID TEXT
  agent-workspace-linux workspace paste-window [--id ID] [--title TEXT] [--class TEXT] [--pid PID] [--app APP_ID_OR_PID_OR_NAME] [--key KEY] [--timeout-ms N] TEXT
  agent-workspace-linux workspace logs [--id ID] [--stream stdout|stderr] [--tail-bytes N] APP_ID_OR_PID_OR_NAME
  agent-workspace-linux workspace wait-app [--id ID] [--timeout-ms N] [--kill-on-timeout] APP_ID_OR_PID_OR_NAME
  agent-workspace-linux workspace events [--id ID] [--tail N] [--since SEQUENCE]
  agent-workspace-linux workspace setup [--id ID] --profile PROFILE [--wait] [--timeout-ms N] [--kill-on-timeout] [--ack-unenforced-policy]
  agent-workspace-linux workspace kill-app [--id ID] APP_ID_OR_PID_OR_NAME
  agent-workspace-linux workspace stop [--id ID] [--timeout-ms N]"#
    );
}
