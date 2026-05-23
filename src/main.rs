mod server;
mod workspace;

use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use workspace::{DaemonOptions, WorkspaceStartOptions};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("doctor") => {
            let report = workspace::doctor_report();
            print_json(&report)
        }
        Some("mcp") => server::serve_mcp().await,
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
            bail!("unknown command '{command}'. Expected one of: doctor, mcp, workspace, --help")
        }
    }
}

fn handle_workspace(args: Vec<String>) -> Result<()> {
    let Some(command) = args.first().map(String::as_str) else {
        bail!("missing workspace command. Expected: start, status, launch, stop");
    };
    match command {
        "start" => {
            let options = parse_start_options(&args[1..])?;
            print_json(&workspace::start_workspace(options)?)
        }
        "status" => {
            let id = parse_id_option(&args[1..])?;
            print_json(&workspace::status_workspace(&id)?)
        }
        "launch" => {
            let (id, command) = parse_launch_options(&args[1..])?;
            print_json(&workspace::launch_app(&id, command)?)
        }
        "stop" => {
            let id = parse_id_option(&args[1..])?;
            print_json(&workspace::stop_workspace(&id)?)
        }
        unknown => {
            bail!("unknown workspace command '{unknown}'. Expected: start, status, launch, stop")
        }
    }
}

fn parse_start_options(args: &[String]) -> Result<WorkspaceStartOptions> {
    let mut options = WorkspaceStartOptions::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                options.id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--width" => {
                options.width = value_after(args, index, "--width")?
                    .parse()
                    .context("--width must be a positive integer")?;
                index += 2;
            }
            "--height" => {
                options.height = value_after(args, index, "--height")?
                    .parse()
                    .context("--height must be a positive integer")?;
                index += 2;
            }
            flag => bail!("unknown workspace start option '{flag}'"),
        }
    }
    Ok(options)
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

fn parse_launch_options(args: &[String]) -> Result<(String, Vec<String>)> {
    let mut id = workspace::default_workspace_id();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = value_after(args, index, "--id")?.to_string();
                index += 2;
            }
            "--" => {
                let command = args[index + 1..].to_vec();
                if command.is_empty() {
                    bail!("workspace launch requires a command after --");
                }
                return Ok((id, command));
            }
            _ => {
                let command = args[index..].to_vec();
                if command.is_empty() {
                    bail!("workspace launch requires a command");
                }
                return Ok((id, command));
            }
        }
    }
    bail!("workspace launch requires a command")
}

fn parse_daemon_options(args: Vec<String>) -> Result<DaemonOptions> {
    let mut id = None;
    let mut display = None;
    let mut width = None;
    let mut height = None;
    let mut runtime_dir = None;
    let mut socket_path = None;
    let mut xauthority_path = None;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--id" => {
                id = Some(value_after(&args, index, "--id")?.to_string());
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
            flag => bail!("unknown daemon option '{flag}'"),
        }
    }

    Ok(DaemonOptions {
        id: id.context("daemon missing --id")?,
        display: display.context("daemon missing --display")?,
        width: width.context("daemon missing --width")?,
        height: height.context("daemon missing --height")?,
        runtime_dir: runtime_dir.context("daemon missing --runtime-dir")?,
        socket_path: socket_path.context("daemon missing --socket")?,
        xauthority_path: xauthority_path.context("daemon missing --xauthority")?,
    })
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
        "agent-workspace-linux\n\nUsage:\n  agent-workspace-linux doctor\n  agent-workspace-linux mcp\n  agent-workspace-linux workspace start [--id ID] [--width PX] [--height PX]\n  agent-workspace-linux workspace status [--id ID]\n  agent-workspace-linux workspace launch [--id ID] -- COMMAND [ARGS...]\n  agent-workspace-linux workspace stop [--id ID]"
    );
}
