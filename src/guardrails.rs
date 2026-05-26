use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GuardrailSummary {
    pub version: u32,
    pub agent_rules: Vec<AgentGuardrailRule>,
    pub acknowledgements: Vec<GuardrailRule>,
    pub previews: Vec<GuardrailRule>,
    pub explicit_overrides: Vec<GuardrailRule>,
    pub policy_modes: Vec<GuardrailRule>,
    pub timeout_terminations: Vec<GuardrailRule>,
    pub scope_notes: Vec<GuardrailRule>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AgentGuardrailRule {
    pub surface: String,
    pub allowed: String,
    pub blocked: String,
    pub requires_ack: String,
    pub exact_parameter: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GuardrailRule {
    pub id: String,
    pub kind: String,
    pub surfaces: Vec<String>,
    pub default_behavior: String,
    pub opt_in: String,
    pub effect: String,
}

pub fn guardrail_summary() -> GuardrailSummary {
    GuardrailSummary {
        version: 15,
        agent_rules: vec![
            agent_rule(
                "workspace_start",
                "dry_run=true previews; real start allowed only when live control is active",
                "real start is blocked without acknowledge_hidden_workspace=true",
                "hidden workspace acknowledgement",
                "acknowledge_hidden_workspace=true",
            ),
            agent_rule(
                "workspace_open_profile",
                "dry_run=true previews start/setup/startup; real open allowed only when live control is active",
                "real open is blocked without hidden-workspace ack and any required policy ack",
                "hidden workspace acknowledgement and possibly unenforced policy acknowledgement",
                "acknowledge_hidden_workspace=true, acknowledge_unenforced_policy=true",
            ),
            agent_rule(
                "profile_put/profile_import/profile_delete",
                "dry_run=true previews profile writes/deletes",
                "real writes/deletes are blocked unless live control is active",
                "user approval before changing the saved profile store",
                "dry_run=true for preview; replace=true only for explicit overwrite",
            ),
            agent_rule(
                "workspace_launch_app/workspace_run_app",
                "dry_run=true previews command/cwd/env/policy; real launch/run allowed only when live control is active",
                "profile policy requests can block until acknowledged or enforced",
                "unenforced policy acknowledgement when profile_check reports it",
                "acknowledge_unenforced_policy=true",
            ),
            agent_rule(
                "workspace_browser_navigate",
                "normal browsing/search navigation is allowed only while live control is active",
                "checkout, purchase, payment, order submission, and account changes stay blocked until separately approved",
                "separate real-world action approval for checkout/account/payment changes",
                "target_id from workspace_browser_targets when multiple pages exist",
            ),
            agent_rule(
                "workspace_open_viewer",
                "allowed while active/read_only/paused when not headless and host display is ready",
                "blocked by --headless or workspace_doctor.ready_for_host_viewer=false",
                "host-visible UI approval when the user has not already asked to watch",
                "always_on_top=true only when explicitly requested",
            ),
            agent_rule(
                "workspace_close_viewer",
                "dry_run=true previews registered viewer cleanup; real close is allowed for repo-owned viewer pids",
                "non-registered host windows are never targeted by this tool",
                "approval before closing visible viewer windows unless it is stale cleanup",
                "id=<viewer_id> or all=true, dry_run=true for preview",
            ),
        ],
        acknowledgements: vec![
            rule(
                "hidden-workspace-start",
                "acknowledgement",
                &[
                    "workspace start",
                    "workspace open-profile",
                    "workspace_start",
                    "workspace_open_profile",
                ],
                "Starting a hidden agent-controlled workspace is rejected.",
                "--ack-hidden-workspace or acknowledge_hidden_workspace=true",
                "Records explicit user awareness that a separate workspace exists outside the visible desktop.",
            ),
            rule(
                "unenforced-profile-policy",
                "acknowledgement",
                &[
                    "workspace start --profile",
                    "workspace open-profile",
                    "workspace launch --profile",
                    "workspace run --profile",
                    "workspace setup",
                    "workspace launch-profile-apps",
                    "profile_check",
                ],
                "Profiles that request mount or network policy not enforced by the runtime are rejected at action time.",
                "--ack-unenforced-policy or acknowledge_unenforced_policy=true",
                "Records that the user accepted running with declared policy that is visible but not fully enforced.",
            ),
        ],
        previews: vec![
            rule(
                "workspace-start-preview",
                "dry_run",
                &["workspace start", "workspace_start"],
                "Workspace start creates a hidden runtime only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Returns acknowledgement, runtime readiness, profile policy, and would-start status without creating X11 runtime state.",
            ),
            rule(
                "profile-open-preview",
                "dry_run",
                &["workspace open-profile", "workspace_open_profile"],
                "Opening a profile starts the workspace and may run setup/startup apps only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Returns start, setup, and startup declarations for approval without creating a workspace or spawning app processes.",
            ),
            rule(
                "workspace-launch-preview",
                "dry_run",
                &["workspace launch", "workspace_launch_app"],
                "Workspace launch spawns a process only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Requires a running workspace daemon, then returns command, cwd/env, profile policy, acknowledgement, isolation, and would-launch status without creating an app record.",
            ),
            rule(
                "workspace-run-preview",
                "dry_run",
                &["workspace run", "workspace_run_app"],
                "Workspace run launches and waits for a process only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Requires a running workspace daemon, then returns launch preview plus timeout, log-tail, and kill-on-timeout options without spawning a process.",
            ),
            rule(
                "profile-startup-launch-preview",
                "dry_run",
                &[
                    "workspace launch-profile-apps",
                    "workspace_launch_profile_apps",
                ],
                "Profile startup apps are launched only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Requires a running workspace daemon, then returns one launch preview per declared startup app without spawning processes.",
            ),
            rule(
                "profile-setup-launch-preview",
                "dry_run",
                &["workspace setup", "workspace_run_profile_setup"],
                "Profile setup commands are launched only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Requires a running workspace daemon, then returns one launch preview per declared setup command without spawning processes.",
            ),
            rule(
                "profile-put-preview",
                "dry_run",
                &["profile put", "profile import", "profile_put", "profile_import"],
                "Profile save writes to the profile store only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Returns whether the profile would be created, replaced, or rejected, including any existing profile.",
            ),
            rule(
                "profile-delete-preview",
                "dry_run",
                &["profile delete", "profile_delete"],
                "Profile delete removes a saved profile only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Returns the profile that would be removed without deleting it.",
            ),
            rule(
                "stale-cleanup-preview",
                "dry_run",
                &["workspace cleanup", "workspace_cleanup_stale"],
                "Cleanup removes stale runtime directories only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Returns stale runtime directory candidates without deleting them.",
            ),
            rule(
                "window-close-preview",
                "dry_run",
                &[
                    "workspace close-window",
                    "workspace_close_window",
                    "workspace_close_matching_window",
                ],
                "Window close requests are sent only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Resolves and returns the targeted window without closing it.",
            ),
            rule(
                "app-kill-preview",
                "dry_run",
                &["workspace kill-app", "workspace_kill_app"],
                "App termination is sent only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Resolves and returns the matched app without terminating it.",
            ),
            rule(
                "workspace-stop-preview",
                "dry_run",
                &["workspace stop", "workspace_stop"],
                "Workspace shutdown and app termination happen only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Returns currently running apps without stopping the workspace.",
            ),
        ],
        explicit_overrides: vec![
            rule(
                "profile-replace",
                "explicit_override",
                &["profile put", "profile import", "profile_put", "profile_import"],
                "Saving a profile with an existing id is rejected.",
                "--replace or replace=true",
                "Allows intentional overwrite of a saved environment profile.",
            ),
            rule(
                "profile-export-replace",
                "explicit_override",
                &["profile export", "profile_export"],
                "Exporting to an existing output file is rejected.",
                "--replace or replace=true",
                "Allows intentional overwrite of an existing exported profile JSON file.",
            ),
        ],
        policy_modes: vec![
            rule(
                "profile-network-inherit-host",
                "policy_mode",
                &["profile network.mode=inherit_host", "profile_check"],
                "No restricted network policy is requested.",
                "No unenforced-policy acknowledgement is needed.",
                "Launched apps use the host network, and profile_check reports network.state=not_requested.",
            ),
            rule(
                "profile-network-disabled",
                "policy_mode",
                &[
                    "profile network.mode=disabled",
                    "profile_check",
                    "workspace start --profile",
                    "workspace launch --profile",
                ],
                "Network-disabled profiles are enforced only when bubblewrap is available.",
                "Install bubblewrap for active --unshare-net enforcement, or pass --ack-unenforced-policy to run without it.",
                "profile_check reports network.state=enforced with backend=bubblewrap_unshare_net, or state=unenforced with required_acknowledgement=ack_unenforced_policy.",
            ),
            rule(
                "profile-network-local-only",
                "policy_mode",
                &[
                    "profile network.mode=local_only",
                    "profile_check",
                    "workspace start --profile",
                    "workspace launch --profile",
                ],
                "Local-only profiles are enforced when bubblewrap is available by launching apps in a loopback-only network namespace.",
                "Install bubblewrap for active loopback-only enforcement, or pass --ack-unenforced-policy to run without it.",
                "profile_check reports network.state=enforced with backend=bubblewrap_loopback_only when active; host localhost services are not bridged into the sandbox.",
            ),
            rule(
                "profile-network-allowlist",
                "policy_mode",
                &[
                    "profile network.mode=allowlist",
                    "profile_check",
                    "workspace start --profile",
                    "workspace launch --profile",
                ],
                "Host allowlists are advanced/legacy declared intent and are outside the current closed/local/open product scope.",
                "--ack-unenforced-policy or acknowledge_unenforced_policy=true",
                "Records acceptance that allow_hosts is visible in policy status but launched apps still keep host network access.",
            ),
            rule(
                "profile-mounts",
                "policy_mode",
                &[
                    "profile mounts",
                    "profile_check",
                    "workspace start --profile",
                    "workspace launch --profile",
                ],
                "Mount profiles are enforced only when bubblewrap is available.",
                "Install bubblewrap for active mount namespace enforcement, or pass --ack-unenforced-policy to run without it.",
                "profile_check reports mounts.state=enforced with backend=bubblewrap_mount_namespace, or state=unenforced with required_acknowledgement=ack_unenforced_policy.",
            ),
            rule(
                "profile-require-enforced-policy",
                "policy_guardrail",
                &[
                    "profile require_enforced_policy=true",
                    "profile_check",
                    "workspace start --profile",
                    "workspace launch --profile",
                ],
                "Profiles may opt into fail-closed policy handling.",
                "Set require_enforced_policy=true in the saved profile.",
                "If any requested mount or network policy is unenforced, starts and launches are rejected even when --ack-unenforced-policy is passed.",
            ),
        ],
        timeout_terminations: vec![
            rule(
                "run-app-kill-on-timeout",
                "explicit_termination",
                &["workspace run", "workspace_run_app"],
                "Timed-out commands are left running unless the caller opts into cleanup.",
                "--kill-on-timeout or kill_on_timeout=true",
                "Terminates the launched app process group when the wait timeout elapses.",
            ),
            rule(
                "wait-app-kill-on-timeout",
                "explicit_termination",
                &["workspace wait-app", "workspace_wait_app"],
                "Timed-out existing apps are left running unless the caller opts into cleanup.",
                "--kill-on-timeout or kill_on_timeout=true",
                "Terminates the matched app process group when the wait timeout elapses.",
            ),
            rule(
                "profile-setup-kill-on-timeout",
                "explicit_termination",
                &[
                    "workspace setup",
                    "workspace open-profile --setup",
                    "workspace_run_profile_setup",
                    "workspace_open_profile",
                ],
                "Timed-out setup commands are left running unless the caller opts into cleanup.",
                "--kill-on-timeout, --setup-kill-on-timeout, kill_on_timeout=true, or setup_kill_on_timeout=true",
                "Terminates timed-out setup command process groups.",
            ),
        ],
        scope_notes: vec![
            rule(
                "workspace-local-actions",
                "scope",
                &[
                    "workspace input tools",
                    "workspace window tools",
                    "workspace app tools",
                ],
                "Tools attach to the isolated workspace display and control socket, not the host desktop.",
                "workspace start creates DISPLAY/XAUTHORITY/AGENT_WORKSPACE_SOCKET for the hidden workspace",
                "Pointer, keyboard, window, clipboard, and app controls are scoped to the isolated X11 workspace.",
            ),
            rule(
                "event-log-redaction",
                "privacy",
                &["workspace events", "workspace_events"],
                "Event logs do not store raw typed text, raw pasted text, or raw clipboard-set text.",
                "workspace events or workspace_events",
                "Sensitive text actions are recorded as metadata such as character counts.",
            ),
        ],
    }
}

fn agent_rule(
    surface: &str,
    allowed: &str,
    blocked: &str,
    requires_ack: &str,
    exact_parameter: &str,
) -> AgentGuardrailRule {
    AgentGuardrailRule {
        surface: surface.to_string(),
        allowed: allowed.to_string(),
        blocked: blocked.to_string(),
        requires_ack: requires_ack.to_string(),
        exact_parameter: exact_parameter.to_string(),
    }
}

fn rule(
    id: &str,
    kind: &str,
    surfaces: &[&str],
    default_behavior: &str,
    opt_in: &str,
    effect: &str,
) -> GuardrailRule {
    GuardrailRule {
        id: id.to_string(),
        kind: kind.to_string(),
        surfaces: surfaces.iter().map(|surface| surface.to_string()).collect(),
        default_behavior: default_behavior.to_string(),
        opt_in: opt_in.to_string(),
        effect: effect.to_string(),
    }
}
