#!/usr/bin/env python3
"""Audit release-only evidence that cannot be proven by unit tests alone."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import ipaddress
import json
import re
import subprocess
import tempfile
from pathlib import Path
from typing import Any
from urllib.parse import urlparse


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_VIEWER_DIR = ROOT / "target" / "viewer-desktop-matrix"
DEFAULT_GROCERY_DIR = ROOT / "target" / "real-grocery-dogfood"
DEFAULT_GITHUB_EXPLORE_DIR = ROOT / "target" / "github-explore-dogfood"
DEFAULT_APP_QA_DIR = ROOT / "target" / "app-qa-dogfood"
DEFAULT_OUTPUT_DIR = ROOT / "target" / "release-gate-audit"
DEFAULT_HUMAN_REVIEW_MARKER = ROOT / "target" / "release-gate-human-review.json"
DEFAULT_DESKTOP_REPO = ROOT.parent / "codex-desktop-linux"
DEFAULT_MAX_EVIDENCE_AGE_DAYS = 14
RUNTIME_SOURCE_IDENTITY_PATHS = ["Cargo.toml", "Cargo.lock", "src", "scripts"]
DESKTOP_SOURCE_IDENTITY_PATHS = [
    "linux-features/agent-workspace",
    "agent-workspaces-linux.js",
]
SOURCE_IDENTITY_PATHS = RUNTIME_SOURCE_IDENTITY_PATHS
REAL_GROCERY_ALLOWED_INPUT_EVENT_KINDS = {
    "click_window",
    "key_window",
    "paste_window",
    "scroll_window",
    "type_window",
}
REAL_GROCERY_ALLOWED_EXECUTED_STEP_ACTIONS = {
    "click_window",
    "key_window",
    "observe",
    "paste_window",
    "scroll_window",
    "type_window",
    "wait_window",
}
REAL_GROCERY_FORBIDDEN_STEP_RE = re.compile(
    r"\b(checkout|place\s+order|submit\s+order|complete\s+order|buy\s+now|"
    r"pay(?:ment)?|card|cvv|account|password|sign\s*up|create\s+account|"
    r"log\s*in|login|subscribe)\b",
    re.IGNORECASE,
)


def skip_local_artifact_path(path: str) -> bool:
    rel = Path(path)
    return "__pycache__" in rel.parts or path.endswith(".pyc")


def load_json_reports(directory: Path) -> list[tuple[Path, dict[str, Any]]]:
    reports: list[tuple[Path, dict[str, Any]]] = []
    if not directory.exists():
        return reports
    for path in sorted(directory.glob("*.json")):
        try:
            value = json.loads(path.read_text(encoding="utf-8"))
        except Exception:
            continue
        if isinstance(value, dict):
            reports.append((path, value))
    return reports


def truthy(value: Any) -> bool:
    return value is True or str(value).strip().lower() in {"1", "true", "yes"}


def run_git(args: list[str], *, cwd: Path = ROOT) -> str | None:
    try:
        completed = subprocess.run(
            ["git", *args],
            cwd=cwd,
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
        )
    except Exception:
        return None
    if completed.returncode != 0:
        return None
    return completed.stdout.strip()


def run_git_bytes(args: list[str], *, cwd: Path) -> bytes | None:
    try:
        completed = subprocess.run(
            ["git", *args],
            cwd=cwd,
            check=False,
            capture_output=True,
            timeout=10,
        )
    except Exception:
        return None
    if completed.returncode != 0:
        return None
    return completed.stdout


def source_file_paths(root: Path, source_paths: list[str]) -> list[str]:
    try:
        completed = subprocess.run(
            [
                "git",
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
                "--",
                *source_paths,
            ],
            cwd=root,
            check=True,
            capture_output=True,
            timeout=5,
        )
        paths = [item.decode("utf-8") for item in completed.stdout.split(b"\0") if item]
    except Exception:
        paths = []
        for source_path in source_paths:
            full_path = root / source_path
            if full_path.is_file():
                paths.append(source_path)
            elif full_path.is_dir():
                paths.extend(
                    str(path.relative_to(root))
                    for path in full_path.rglob("*")
                    if path.is_file()
                )
    return sorted(
        path for path in paths if not skip_local_artifact_path(path)
    )


def compute_repo_source_identity(
    root: Path,
    source_paths: list[str],
    *,
    label: str,
) -> dict[str, Any]:
    if not root.exists():
        return {
            "label": label,
            "path": str(root),
            "exists": False,
            "git_head": None,
            "source_hash": None,
            "source_paths": source_paths,
            "source_file_count": 0,
            "source_dirty_count": None,
        }
    hasher = hashlib.sha256()
    paths = source_file_paths(root, source_paths)
    for rel_path in paths:
        full_path = root / rel_path
        if not full_path.is_file():
            continue
        hasher.update(rel_path.encode("utf-8"))
        hasher.update(b"\0")
        hasher.update(full_path.read_bytes())
        hasher.update(b"\0")
    dirty_output = run_git(["status", "--porcelain=v1", "--", *source_paths], cwd=root) or ""
    return {
        "label": label,
        "path": str(root),
        "exists": True,
        "git_head": run_git(["rev-parse", "HEAD"], cwd=root),
        "source_hash": hasher.hexdigest(),
        "source_paths": source_paths,
        "source_file_count": len(paths),
        "source_dirty_count": len([line for line in dirty_output.splitlines() if line.strip()]),
    }


def compute_runtime_source_identity(root: Path = ROOT) -> dict[str, Any]:
    return compute_repo_source_identity(
        root,
        RUNTIME_SOURCE_IDENTITY_PATHS,
        label="runtime",
    )


def compute_desktop_source_identity(desktop_repo: Path = DEFAULT_DESKTOP_REPO) -> dict[str, Any]:
    return compute_repo_source_identity(
        desktop_repo,
        DESKTOP_SOURCE_IDENTITY_PATHS,
        label="codex_desktop",
    )


def combine_source_identities(
    runtime_identity: dict[str, Any],
    desktop_identity: dict[str, Any],
) -> dict[str, Any]:
    components = {
        "runtime": runtime_identity,
        "codex_desktop": desktop_identity,
    }
    hash_payload = {
        name: {
            "exists": component.get("exists") is True,
            "git_head": component.get("git_head"),
            "source_hash": component.get("source_hash"),
            "source_file_count": component.get("source_file_count"),
            "source_paths": component.get("source_paths"),
        }
        for name, component in components.items()
    }
    combined_hash = hashlib.sha256(
        json.dumps(hash_payload, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()
    dirty_counts = [
        component.get("source_dirty_count")
        for component in components.values()
        if component.get("exists") is True
    ]
    source_dirty_count = (
        sum(int(count) for count in dirty_counts if isinstance(count, int))
        if all(isinstance(count, int) for count in dirty_counts)
        else None
    )
    missing_components = [
        name for name, component in components.items() if component.get("exists") is not True
    ]
    git_head = "|".join(
        f"{name}:{component.get('git_head') if component.get('exists') else 'missing'}"
        for name, component in components.items()
    )
    return {
        "schema": "agent-workspace-linux.source_identity.v2",
        "git_head": git_head,
        "source_hash": combined_hash,
        "source_paths": {
            "runtime": RUNTIME_SOURCE_IDENTITY_PATHS,
            "codex_desktop": DESKTOP_SOURCE_IDENTITY_PATHS,
        },
        "source_file_count": sum(
            int(component.get("source_file_count") or 0) for component in components.values()
        ),
        "source_dirty_count": source_dirty_count,
        "missing_components": missing_components,
        "components": components,
    }


def compute_source_identity(
    root: Path = ROOT,
    *,
    desktop_repo: Path = DEFAULT_DESKTOP_REPO,
) -> dict[str, Any]:
    return combine_source_identities(
        compute_runtime_source_identity(root),
        compute_desktop_source_identity(desktop_repo),
    )


def bundle_source_content_errors(
    manifest: dict[str, Any],
    *,
    root: Path = ROOT,
    desktop_repo: Path = DEFAULT_DESKTOP_REPO,
) -> list[str]:
    expected_components = ((manifest.get("source_identity") or {}).get("components") or {})
    if not isinstance(expected_components, dict):
        return ["release bundle manifest source_identity.components is invalid"]
    actual_components = {
        "runtime": compute_runtime_source_identity(root),
        "codex_desktop": compute_desktop_source_identity(desktop_repo),
    }
    errors: list[str] = []
    for name, actual in actual_components.items():
        expected = expected_components.get(name)
        if not isinstance(expected, dict):
            errors.append(f"missing {name} source component in release bundle manifest")
            continue
        if actual.get("exists") is not expected.get("exists"):
            errors.append(
                f"{name} exists={actual.get('exists')} does not match manifest exists={expected.get('exists')}"
            )
            continue
        if expected.get("exists") is not True:
            continue
        for key in ["source_hash", "source_file_count"]:
            if actual.get(key) != expected.get(key):
                errors.append(
                    f"{name} {key}={actual.get(key)} does not match manifest {key}={expected.get(key)}"
                )
    return errors


def validate_bundle_manifest_source_contents(
    manifest: dict[str, Any],
    *,
    root: Path = ROOT,
    desktop_repo: Path = DEFAULT_DESKTOP_REPO,
) -> None:
    errors = bundle_source_content_errors(manifest, root=root, desktop_repo=desktop_repo)
    if errors:
        raise RuntimeError(
            "release bundle source bytes no longer match the manifest; "
            f"refusing to stamp manifest source identity: {'; '.join(errors)}"
        )


def git_untracked_paths(root: Path) -> list[str]:
    output = run_git_bytes(["ls-files", "-z", "--others", "--exclude-standard"], cwd=root)
    if output is None:
        return []
    return sorted(
        item.decode("utf-8")
        for item in output.split(b"\0")
        if item and not skip_local_artifact_path(item.decode("utf-8"))
    )


def git_status_lines(root: Path) -> list[str]:
    output = run_git(["status", "--porcelain=v1"], cwd=root) or ""
    lines = []
    for line in output.splitlines():
        if not line.strip():
            continue
        path_text = line[3:].strip()
        paths = [part.strip() for part in path_text.split(" -> ")]
        if paths and all(skip_local_artifact_path(path) for path in paths):
            continue
        lines.append(line)
    return lines


def compute_repo_review_scope_identity(root: Path, *, label: str) -> dict[str, Any]:
    if not root.exists():
        return {
            "label": label,
            "path": str(root),
            "exists": False,
            "git_head": None,
            "review_scope_hash": None,
            "dirty_count": None,
            "untracked_file_count": 0,
        }

    hasher = hashlib.sha256()
    dirty_lines = git_status_lines(root)
    review_mode = "dirty_worktree" if dirty_lines else "clean_head"
    status_bytes = ("\n".join(dirty_lines) + "\n").encode("utf-8")
    unstaged_diff = b""
    staged_diff = b""
    head_commit = b""
    if dirty_lines:
        unstaged_diff = run_git_bytes(["diff", "--binary", "--no-ext-diff"], cwd=root) or b""
        staged_diff = run_git_bytes(["diff", "--binary", "--cached", "--no-ext-diff"], cwd=root) or b""
    else:
        head_commit = (
            run_git_bytes(["show", "--binary", "--no-ext-diff", "--format=fuller", "HEAD"], cwd=root)
            or b""
        )
    untracked_paths = git_untracked_paths(root)

    for name, payload in [
        ("label", label.encode("utf-8")),
        ("review_mode", review_mode.encode("utf-8")),
        ("status", status_bytes),
        ("unstaged_diff", unstaged_diff),
        ("staged_diff", staged_diff),
        ("head_commit", head_commit),
    ]:
        hasher.update(name.encode("utf-8"))
        hasher.update(b"\0")
        hasher.update(payload)
        hasher.update(b"\0")

    for rel_path in untracked_paths:
        full_path = root / rel_path
        if not full_path.is_file():
            continue
        hasher.update(b"untracked")
        hasher.update(b"\0")
        hasher.update(rel_path.encode("utf-8"))
        hasher.update(b"\0")
        hasher.update(full_path.read_bytes())
        hasher.update(b"\0")

    return {
        "label": label,
        "path": str(root),
        "exists": True,
        "git_head": run_git(["rev-parse", "HEAD"], cwd=root),
        "review_mode": review_mode,
        "review_scope_hash": hasher.hexdigest(),
        "dirty_count": len(dirty_lines),
        "untracked_file_count": len(untracked_paths),
    }


def compute_review_scope_identity(
    root: Path = ROOT,
    *,
    desktop_repo: Path = DEFAULT_DESKTOP_REPO,
) -> dict[str, Any]:
    components = {
        "runtime": compute_repo_review_scope_identity(root, label="runtime"),
        "codex_desktop": compute_repo_review_scope_identity(
            desktop_repo,
            label="codex_desktop",
        ),
    }
    hash_payload = {
        name: {
            "exists": component.get("exists") is True,
            "git_head": component.get("git_head"),
            "review_mode": component.get("review_mode"),
            "review_scope_hash": component.get("review_scope_hash"),
            "dirty_count": component.get("dirty_count"),
            "untracked_file_count": component.get("untracked_file_count"),
        }
        for name, component in components.items()
    }
    review_scope_hash = hashlib.sha256(
        json.dumps(hash_payload, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()
    dirty_counts = [
        component.get("dirty_count")
        for component in components.values()
        if component.get("exists") is True
    ]
    dirty_count = (
        sum(int(count) for count in dirty_counts if isinstance(count, int))
        if all(isinstance(count, int) for count in dirty_counts)
        else None
    )
    missing_components = [
        name for name, component in components.items() if component.get("exists") is not True
    ]
    git_head = "|".join(
        f"{name}:{component.get('git_head') if component.get('exists') else 'missing'}"
        for name, component in components.items()
    )
    return {
        "schema": "agent-workspace-linux.review_scope_identity.v1",
        "git_head": git_head,
        "review_scope_hash": review_scope_hash,
        "dirty_count": dirty_count,
        "missing_components": missing_components,
        "components": components,
    }


def source_identity_matches(report: dict[str, Any], expected: dict[str, Any] | None) -> bool:
    if expected is None:
        return True
    identity = report.get("source_identity") or {}
    return (
        isinstance(identity, dict)
        and identity.get("source_hash") == expected.get("source_hash")
        and identity.get("git_head") == expected.get("git_head")
    )


def repo_owned_runtime_evidence(report: dict[str, Any]) -> bool:
    boundary = report.get("evidence_boundary") or {}
    return (
        isinstance(boundary, dict)
        and boundary.get("collector") == "agent-workspace-linux"
        and boundary.get("repo_owned_runtime") is True
        and boundary.get("codex_app_mcp_used") is False
        and boundary.get("computer_use_mcp_used") is False
        and boundary.get("codex_desktop_bridge_used") is False
        and boundary.get("playwright_mcp_used") is False
    )


def review_scope_matches(marker: dict[str, Any], expected: dict[str, Any] | None) -> bool:
    if expected is None:
        return True
    identity = marker.get("review_scope_identity") or {}
    return (
        isinstance(identity, dict)
        and identity.get("review_scope_hash") == expected.get("review_scope_hash")
        and identity.get("git_head") == expected.get("git_head")
    )


def file_sha256(path: Path) -> str | None:
    if not path.is_file():
        return None
    digest = hashlib.sha256()
    try:
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
    except OSError:
        return None
    return digest.hexdigest()


def review_artifacts_match(marker: dict[str, Any]) -> bool:
    artifacts = marker.get("review_artifacts")
    if not isinstance(artifacts, list):
        return False
    by_label = {
        artifact.get("label"): artifact
        for artifact in artifacts
        if isinstance(artifact, dict) and isinstance(artifact.get("label"), str)
    }
    if {"runtime", "codex_desktop"} - set(by_label):
        return False
    for label in ["runtime", "codex_desktop"]:
        artifact = by_label[label]
        path_value = artifact.get("path")
        expected_sha = artifact.get("sha256")
        expected_size = artifact.get("size_bytes")
        if (
            not isinstance(path_value, str)
            or not re.fullmatch(r"[0-9a-f]{64}", str(expected_sha or ""))
            or not isinstance(expected_size, int)
            or expected_size <= 0
        ):
            return False
        path = Path(path_value)
        if not path.is_file() or path.stat().st_size != expected_size:
            return False
        if file_sha256(path) != expected_sha:
            return False
    return True


def meaningful_human_review_text(value: Any) -> bool:
    if not isinstance(value, str):
        return False
    normalized = " ".join(value.strip().split())
    if len(normalized) < 8:
        return False
    lowered = normalized.lower()
    placeholder_needles = [
        "<",
        ">",
        "what was reviewed",
        "scope, concerns",
        "fill with",
        "todo",
        "tbd",
        "final runtime and sibling desktop diffs reviewed.",
    ]
    return not any(needle in lowered for needle in placeholder_needles)


def human_review_metadata_ok(marker: dict[str, Any]) -> bool:
    return meaningful_human_review_text(marker.get("reviewer")) and meaningful_human_review_text(
        marker.get("notes")
    )


def parse_timestamp(value: Any) -> dt.datetime | None:
    if not isinstance(value, str) or not value.strip():
        return None
    try:
        parsed = dt.datetime.fromisoformat(value.strip().replace("Z", "+00:00"))
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return parsed.replace(tzinfo=dt.timezone.utc)
    return parsed.astimezone(dt.timezone.utc)


def evidence_timestamp(report: dict[str, Any]) -> dt.datetime | None:
    return parse_timestamp(report.get("created_at_utc") or report.get("reviewed_at_utc"))


def is_fresh_evidence(
    report: dict[str, Any],
    *,
    now: dt.datetime,
    max_evidence_age_days: int,
) -> bool:
    if max_evidence_age_days <= 0:
        return True
    created_at = evidence_timestamp(report)
    if created_at is None:
        return False
    age = now - created_at
    return dt.timedelta(0) <= age <= dt.timedelta(days=max_evidence_age_days)


def desktop_text(report: dict[str, Any]) -> str:
    session = report.get("session") or {}
    matrix = report.get("matrix_result") or {}
    parts = [
        session.get("xdg_current_desktop"),
        session.get("desktop_session"),
        matrix.get("desktop_label"),
    ]
    return " ".join(str(part) for part in parts if part).lower()


def session_type(report: dict[str, Any]) -> str:
    session = report.get("session") or {}
    return str(session.get("xdg_session_type") or "").lower()


def native_wayland_observed(report: dict[str, Any]) -> bool:
    matrix = report.get("matrix_result") or {}
    notes = str(matrix.get("native_wayland_layer_shell_notes") or "").strip()
    normalized_notes = notes.lower()
    desktop = desktop_text(report)
    positive_needles = [
        "layer-shell",
        "layer shell",
        "top-layer",
        "top layer",
        "overlay layer",
        "layer::overlay",
    ]
    negative_needles = [
        "not layer-shell",
        "not layer shell",
        "not a compositor layer",
        "not an embedded layer",
        "normal resizable xwayland",
        "xwayland toplevel",
        "x11/xwayland",
        "x11 wm state",
        "x11 window",
    ]
    return (
        session_type(report) == "wayland"
        and truthy(matrix.get("native_wayland_layer_shell_observed"))
        and bool(notes)
        and "gnome" not in desktop
        and any(needle in normalized_notes for needle in positive_needles)
        and not any(needle in normalized_notes for needle in negative_needles)
    )


def display_protocol_has_process(report: dict[str, Any], protocol: str) -> bool:
    matrix = report.get("matrix_result") or {}
    display = matrix.get("display_attestation")
    if not isinstance(display, dict):
        return False
    sockets = display.get("sockets")
    if not isinstance(sockets, list):
        return False
    for socket in sockets:
        if not isinstance(socket, dict):
            continue
        if socket.get("kind") != protocol or socket.get("exists") is not True:
            continue
        processes = socket.get("processes")
        if not isinstance(processes, list):
            continue
        for process in processes:
            if (
                isinstance(process, dict)
                and isinstance(process.get("pid"), int)
                and process.get("pid") > 0
                and str(process.get("command") or "").strip()
            ):
                return True
    return False


def x11_xwayland_viewer_protocol_observed(report: dict[str, Any]) -> bool:
    viewer_smoke = report.get("viewer_smoke") or {}
    matrix = report.get("matrix_result") or {}
    summary = viewer_smoke.get("summary")
    if not isinstance(summary, dict):
        return False
    default_viewer = summary.get("default_viewer") or {}
    topmost_viewer = summary.get("topmost_viewer") or {}
    duplicate_launch = summary.get("duplicate_launch") or {}
    return (
        viewer_smoke.get("status") == "passed"
        and matrix.get("x11_xwayland_viewer_protocol_observed") is True
        and summary.get("schema") == "agent-workspace-linux.gpui_viewer_smoke_summary.v1"
        and summary.get("viewer_backend_forced") == "x11"
        and summary.get("x11_xwayland_window_observed") is True
        and display_protocol_has_process(report, "x11")
        and default_viewer.get("skip_taskbar") is True
        and default_viewer.get("skip_pager") is True
        and default_viewer.get("above") is False
        and default_viewer.get("sticky") is False
        and default_viewer.get("notification_or_utility") is True
        and duplicate_launch.get("reused_existing_instance") is True
        and topmost_viewer.get("above") is True
        and topmost_viewer.get("sticky") is True
        and summary.get("target_bound_viewer_exited_after_workspace_cleanup") is True
    )


def viewer_matrix_session_release_eligible(report: dict[str, Any]) -> bool:
    matrix = report.get("matrix_result") or {}
    consistency = matrix.get("session_consistency")
    if isinstance(consistency, dict) and consistency.get("release_eligible") is False:
        return False
    expected_protocol = session_type(report)
    if expected_protocol not in {"x11", "wayland"}:
        return False
    display = matrix.get("display_attestation")
    if not isinstance(display, dict):
        return False
    if display.get("release_eligible") is not True:
        return False
    if display.get("problems") not in ([], None):
        return False
    nested = display.get("known_nested_or_headless_processes")
    if isinstance(nested, list) and nested:
        return False
    protocols = display.get("display_protocols")
    if not isinstance(protocols, list) or expected_protocol not in {
        str(protocol) for protocol in protocols
    }:
        return False
    if display.get("lsof_available") is not True:
        return False
    sockets = display.get("sockets")
    if not isinstance(sockets, list):
        return False
    for socket in sockets:
        if not isinstance(socket, dict):
            continue
        if socket.get("kind") != expected_protocol or socket.get("exists") is not True:
            continue
        processes = socket.get("processes")
        if not isinstance(processes, list):
            continue
        for process in processes:
            if (
                isinstance(process, dict)
                and isinstance(process.get("pid"), int)
                and process.get("pid") > 0
                and str(process.get("command") or "").strip()
            ):
                return True
    return False


def real_grocery_target_url(report: dict[str, Any]) -> bool:
    inputs = report.get("inputs") or {}
    value = str(inputs.get("target_url") or "").strip()
    try:
        parsed = urlparse(value)
    except Exception:
        return False
    hostname = (parsed.hostname or "").strip().lower().rstrip(".")
    if parsed.scheme != "https" or not hostname:
        return False
    if hostname in {"localhost", "example.com", "example.net", "example.org"}:
        return False
    if hostname.endswith((".localhost", ".local", ".test", ".invalid", ".example")):
        return False
    try:
        address = ipaddress.ip_address(hostname)
    except ValueError:
        return True
    return address.is_global


def safe_real_grocery_profile_directory(value: Any) -> bool:
    if not isinstance(value, str):
        return False
    if not value or value != value.strip():
        return False
    if value in {".", ".."}:
        return False
    if "/" in value or "\\" in value or "\0" in value or ".." in value:
        return False
    return all(ord(ch) >= 32 for ch in value)


def real_grocery_profile_directory_ok(report: dict[str, Any]) -> bool:
    inputs_value = report.get("inputs") or {}
    real_browser_value = report.get("real_browser") or {}
    inputs = inputs_value if isinstance(inputs_value, dict) else {}
    real_browser = real_browser_value if isinstance(real_browser_value, dict) else {}
    manifest_value = real_browser.get("profile_copy_manifest") or {}
    manifest = manifest_value if isinstance(manifest_value, dict) else {}
    raw_values = [
        inputs.get("profile_directory"),
        real_browser.get("profile_directory"),
        manifest.get("profile_directory"),
    ]
    if all(value is None or value == "" for value in raw_values):
        return True
    if any(not safe_real_grocery_profile_directory(value) for value in raw_values):
        return False
    return len(set(raw_values)) == 1


def real_grocery_safety_contract_ok(report: dict[str, Any]) -> bool:
    safety = report.get("safety_contract") or {}
    interaction_mode = safety.get("real_browser_interaction_mode")
    return (
        safety.get("refuses_checkout_or_real_world_approval") is True
        and safety.get("real_browser_requires_disposable_profile_copy") is True
        and safety.get("checkout_order_or_account_change_blocked") is True
        and safety.get("cart_draft_requires_explicit_approval") is True
        and interaction_mode == "cart-draft-approved"
        and safety.get("real_browser_allows_only_declared_cart_draft_input") is True
        and safety.get("real_browser_cleans_workspace_runtime") is True
    )


def real_grocery_workspace_cleanup_ok(report: dict[str, Any]) -> bool:
    real_browser = report.get("real_browser") or {}
    workspace_id = real_browser.get("workspace_id")
    cleanup = real_browser.get("cleanup") or {}
    if not isinstance(workspace_id, str) or not workspace_id.strip():
        return False
    if real_browser.get("workspace_preserved_for_debug") is True:
        return False
    if not isinstance(cleanup, dict) or cleanup.get("dry_run") is not False:
        return False
    skipped = cleanup.get("skipped")
    if isinstance(skipped, list):
        for entry in skipped:
            if entry == workspace_id or (
                isinstance(entry, dict) and entry.get("id") == workspace_id
            ):
                return False
    removed = cleanup.get("removed")
    if not isinstance(removed, list):
        return False
    return any(
        entry == workspace_id
        or (isinstance(entry, dict) and entry.get("id") == workspace_id)
        for entry in removed
    )


def real_grocery_plan_assertions_ok(report: dict[str, Any]) -> bool:
    assertions = report.get("plan_assertions") or {}
    return (
        assertions.get("status") == "passed"
        and assertions.get("checkout_still_blocked_after_cart_approval") is True
        and isinstance(assertions.get("unapproved_next_boundary"), dict)
        and isinstance(assertions.get("cart_only_next_boundary"), dict)
    )


def loopback_url(value: Any, *, schemes: set[str]) -> bool:
    if not isinstance(value, str) or not value.strip():
        return False
    try:
        parsed = urlparse(value)
    except Exception:
        return False
    if parsed.scheme not in schemes:
        return False
    hostname = (parsed.hostname or "").strip().lower()
    if hostname == "localhost":
        return True
    try:
        return ipaddress.ip_address(hostname).is_loopback
    except ValueError:
        return False


def same_site_host(actual_url: Any, expected_url: Any) -> bool:
    if not isinstance(actual_url, str) or not isinstance(expected_url, str):
        return False
    try:
        actual = urlparse(actual_url)
        expected = urlparse(expected_url)
    except Exception:
        return False
    actual_host = (actual.hostname or "").strip().lower().rstrip(".")
    expected_host = (expected.hostname or "").strip().lower().rstrip(".")
    return bool(
        actual_host
        and expected_host
        and (
            actual_host == expected_host
            or actual_host.endswith(f".{expected_host}")
            or expected_host.endswith(f".{actual_host}")
        )
    )


def real_grocery_workspace_browser_control_ok(report: dict[str, Any]) -> bool:
    inputs = report.get("inputs") or {}
    real_browser = report.get("real_browser") or {}
    devtools = real_browser.get("chrome_devtools") or {}
    discovered = devtools.get("workspace_browser_targets") or {}
    snapshot = devtools.get("workspace_browser_snapshot") or {}
    page_snapshot = devtools.get("page_snapshot") or {}
    selected = discovered.get("selected_page_target") or {}
    target = devtools.get("target") or {}
    browser_app_id = real_browser.get("browser_app_id")
    endpoint = discovered.get("devtools_endpoint") or devtools.get("endpoint")
    return (
        devtools.get("status") == "passed"
        and devtools.get("control_surface") == "workspace_chrome_devtools"
        and devtools.get("workspace_owned_browser") is True
        and devtools.get("host_chrome_bridge_used") is False
        and devtools.get("coordinate_input_used") is False
        and discovered.get("ok") is True
        and isinstance(browser_app_id, str)
        and bool(browser_app_id.strip())
        and discovered.get("app_id") == browser_app_id
        and loopback_url(endpoint, schemes={"http"})
        and isinstance(discovered.get("target_count"), int)
        and discovered.get("target_count") > 0
        and selected.get("type") == "page"
        and loopback_url(selected.get("webSocketDebuggerUrl"), schemes={"ws", "wss"})
        and same_site_host(selected.get("url") or target.get("url"), inputs.get("target_url"))
        and snapshot.get("ok") is True
        and snapshot.get("app_id") == browser_app_id
        and snapshot.get("target_id") == selected.get("id")
        and same_site_host(
            snapshot.get("page_url") or page_snapshot.get("url"),
            inputs.get("target_url"),
        )
        and isinstance(snapshot.get("text_chars"), int)
        and snapshot.get("text_chars") >= 0
        and real_grocery_snapshot_privacy_ok(page_snapshot)
    )


def real_grocery_snapshot_privacy_ok(page_snapshot: dict[str, Any]) -> bool:
    if not isinstance(page_snapshot, dict):
        return False
    raw_text_fields = ("text", "text_excerpt", "links", "headings")
    for field in raw_text_fields:
        value = page_snapshot.get(field)
        if isinstance(value, str) and value.strip():
            return False
        if isinstance(value, list) and value:
            return False
        if isinstance(value, dict) and value:
            return False
    return (
        page_snapshot.get("raw_text_omitted") is True
        and isinstance(page_snapshot.get("text_chars"), int)
        and page_snapshot.get("text_chars") >= 0
    )


def real_grocery_cart_draft_steps_manifest_ok(report: dict[str, Any]) -> bool:
    inputs = report.get("inputs") or {}
    real_browser = report.get("real_browser") or {}
    manifest = real_browser.get("cart_draft_steps") or {}
    interaction = real_browser.get("cart_draft_interaction") or {}
    if not isinstance(inputs, dict) or not isinstance(manifest, dict) or not isinstance(interaction, dict):
        return False
    path_value = manifest.get("path")
    sha256 = manifest.get("sha256")
    size_bytes = manifest.get("size_bytes")
    if not isinstance(path_value, str) or not path_value.strip():
        return False
    if inputs.get("cart_draft_steps_path") != path_value or interaction.get("steps_path") != path_value:
        return False
    if not isinstance(sha256, str) or re.fullmatch(r"[0-9a-f]{64}", sha256) is None:
        return False
    if interaction.get("steps_sha256") != sha256:
        return False
    if not isinstance(size_bytes, int) or size_bytes <= 0:
        return False
    if interaction.get("steps_size_bytes") != size_bytes:
        return False
    for key in ["step_count", "input_step_count", "cart_mutation_step_count"]:
        value = manifest.get(key)
        if not isinstance(value, int) or value <= 0 or interaction.get(key) != value:
            return False
    summaries = manifest.get("summaries")
    executed_steps = interaction.get("executed_steps")
    step_count = manifest.get("step_count")
    if not isinstance(summaries, list) or not isinstance(executed_steps, list):
        return False
    if len(summaries) != step_count or len(executed_steps) != step_count:
        return False
    for summary, executed in zip(summaries, executed_steps):
        if not isinstance(summary, dict) or not isinstance(executed, dict):
            return False
        for key in ["index", "action", "safety_label", "cart_mutation", "text_bytes"]:
            if summary.get(key) != executed.get(key):
                return False
    return True


def real_grocery_workspace_input_audit_ok(report: dict[str, Any]) -> bool:
    real_browser = report.get("real_browser") or {}
    interaction = real_browser.get("cart_draft_interaction") or {}
    audit = real_browser.get("workspace_input_audit") or {}
    allowed = audit.get("allowed_input_event_kinds")
    kinds = audit.get("input_event_kinds")
    sequences = audit.get("input_event_sequences")
    input_event_count = audit.get("input_event_count")
    expected_input_step_count = audit.get("expected_input_step_count")
    events_tail_requested = audit.get("events_tail_requested")
    minimum_events_tail_required = audit.get("minimum_events_tail_required")
    events_since_sequence = audit.get("events_since_sequence")
    if not isinstance(allowed, list) or not isinstance(kinds, list):
        return False
    if not isinstance(sequences, list):
        return False
    allowed_set = set(str(kind) for kind in allowed)
    kinds_set = set(str(kind) for kind in kinds)
    return (
        audit.get("checked") is True
        and audit.get("event_scope") == "since_sequence"
        and isinstance(events_since_sequence, int)
        and events_since_sequence >= 0
        and all(isinstance(sequence, int) and sequence > events_since_sequence for sequence in sequences)
        and isinstance(input_event_count, int)
        and input_event_count > 0
        and isinstance(expected_input_step_count, int)
        and expected_input_step_count > 0
        and expected_input_step_count == interaction.get("input_step_count")
        and input_event_count >= expected_input_step_count
        and audit.get("input_event_count_covers_expected") is True
        and isinstance(events_tail_requested, int)
        and isinstance(minimum_events_tail_required, int)
        and minimum_events_tail_required >= expected_input_step_count
        and events_tail_requested >= minimum_events_tail_required
        and audit.get("unexpected_input_event_count") == 0
        and allowed_set == REAL_GROCERY_ALLOWED_INPUT_EVENT_KINDS
        and kinds_set.issubset(allowed_set)
    )


def real_grocery_executed_steps_ok(interaction: dict[str, Any]) -> bool:
    steps = interaction.get("executed_steps")
    if not isinstance(steps, list) or not steps:
        return False
    step_count = interaction.get("step_count")
    if not isinstance(step_count, int) or step_count != len(steps):
        return False
    cart_mutations = 0
    input_steps = 0
    for step in steps:
        if not isinstance(step, dict):
            return False
        result = step.get("result")
        if not isinstance(result, dict) or result.get("ok") is not True:
            return False
        action = str(step.get("action") or "")
        if action not in REAL_GROCERY_ALLOWED_EXECUTED_STEP_ACTIONS:
            return False
        safety_label = str(step.get("safety_label") or "")
        if action in REAL_GROCERY_ALLOWED_INPUT_EVENT_KINDS:
            input_steps += 1
            if not safety_label:
                return False
        if REAL_GROCERY_FORBIDDEN_STEP_RE.search(safety_label):
            return False
        if step.get("cart_mutation") is True:
            cart_mutations += 1
            if action not in REAL_GROCERY_ALLOWED_INPUT_EVENT_KINDS:
                return False
    return (
        input_steps == interaction.get("input_step_count")
        and cart_mutations == interaction.get("cart_mutation_step_count")
        and cart_mutations > 0
    )


def real_grocery_cart_draft_interaction_ok(report: dict[str, Any]) -> bool:
    real_browser = report.get("real_browser") or {}
    interaction = real_browser.get("cart_draft_interaction") or {}
    return (
        real_browser.get("interaction_mode") == "cart-draft-approved"
        and interaction.get("status") == "passed"
        and interaction.get("mode") == "cart-draft-approved"
        and interaction.get("cart_mutation_approval_confirmed") is True
        and interaction.get("final_cart_reviewed_confirmed") is True
        and interaction.get("checkout_or_real_world_approval_refused") is True
        and isinstance(interaction.get("input_step_count"), int)
        and interaction.get("input_step_count") > 0
        and isinstance(interaction.get("cart_mutation_step_count"), int)
        and interaction.get("cart_mutation_step_count") > 0
        and interaction.get("forbidden_step_count") == 0
        and real_grocery_executed_steps_ok(interaction)
    )


def github_explore_target_url(report: dict[str, Any]) -> bool:
    inputs = report.get("inputs") or {}
    value = str(inputs.get("target_url") or "").strip()
    try:
        parsed = urlparse(value)
    except Exception:
        return False
    hostname = (parsed.hostname or "").strip().lower().rstrip(".")
    if parsed.scheme != "https" or hostname != "github.com":
        return False
    return parsed.path == "/explore" or parsed.path.startswith(
        ("/topics", "/trending", "/collections")
    )


def github_repo_url_matches(full_name: Any, url: Any) -> bool:
    if not isinstance(full_name, str) or not isinstance(url, str):
        return False
    if re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", full_name) is None:
        return False
    try:
        parsed = urlparse(url)
    except Exception:
        return False
    return (
        parsed.scheme == "https"
        and (parsed.hostname or "").lower().rstrip(".") == "github.com"
        and parsed.path.strip("/") == full_name
    )


def github_explore_recommendations_ok(report: dict[str, Any]) -> bool:
    recommendations = report.get("recommendations")
    if not isinstance(recommendations, list) or len(recommendations) < 3:
        return False
    for recommendation in recommendations[:3]:
        if not isinstance(recommendation, dict):
            return False
        if not github_repo_url_matches(
            recommendation.get("full_name"), recommendation.get("url")
        ):
            return False
        if not str(recommendation.get("reason") or "").strip():
            return False
        matched = recommendation.get("matched_terms")
        if not isinstance(matched, list) or not matched:
            return False
    return report.get("recommendation_count") == len(recommendations)


def github_explore_safety_contract_ok(report: dict[str, Any]) -> bool:
    safety = report.get("safety_contract") or {}
    return (
        safety.get("public_repository_discovery_only") is True
        and safety.get("no_host_browser_bridge") is True
        and safety.get("no_playwright_or_curl") is True
        and safety.get("no_account_mutation") is True
        and safety.get("raw_page_text_omitted_from_report") is True
    )


def github_explore_viewer_metadata_ok(report: dict[str, Any]) -> bool:
    browser = report.get("workspace_browser") or {}
    viewer = browser.get("viewer") or {}
    launch = viewer.get("launch") or {}
    return (
        viewer.get("ok") is True
        and isinstance(launch, dict)
        and isinstance(launch.get("id"), str)
        and launch.get("id") == browser.get("workspace_id")
        and isinstance(launch.get("pid"), int)
        and launch.get("pid") > 0
        and isinstance(launch.get("backend"), str)
        and launch.get("always_on_top") is True
        and launch.get("exit_when_workspace_gone") is True
        and isinstance(launch.get("command"), list)
        and any("viewer" == str(part) for part in launch.get("command") or [])
    )


def github_explore_workspace_browser_ok(report: dict[str, Any]) -> bool:
    browser = report.get("workspace_browser") or {}
    endpoint = browser.get("devtools_endpoint")
    return (
        browser.get("status") == "passed"
        and browser.get("control_surface") == "direct_mcp_workspace_browser_devtools"
        and browser.get("workspace_owned_browser") is True
        and browser.get("host_chrome_bridge_used") is False
        and browser.get("coordinate_input_used") is False
        and isinstance(browser.get("workspace_id"), str)
        and bool(browser.get("workspace_id").strip())
        and isinstance(browser.get("browser_app_id"), str)
        and bool(browser.get("browser_app_id").strip())
        and same_site_host(browser.get("page_url"), "https://github.com/explore")
        and isinstance(browser.get("target_id"), str)
        and bool(browser.get("target_id").strip())
        and isinstance(browser.get("target_count"), int)
        and browser.get("target_count") > 0
        and loopback_url(endpoint, schemes={"http"})
        and isinstance(browser.get("snapshot_text_chars"), int)
        and browser.get("snapshot_text_chars") > 0
        and isinstance(browser.get("launch_screenshot_bytes"), int)
        and browser.get("launch_screenshot_bytes") > 0
        and isinstance(browser.get("event_count"), int)
        and browser.get("event_count") > 0
        and isinstance(browser.get("cleanup"), dict)
        and browser.get("cleanup", {}).get("ok") is True
        and github_explore_viewer_metadata_ok(report)
    )


def github_explore_dogfood_contract_ok(report: dict[str, Any]) -> bool:
    return (
        report.get("schema") == "agent-workspace-linux.github_explore_dogfood.v1"
        and report.get("mode") == "workspace-github-explore"
        and report.get("status") == "passed"
        and repo_owned_runtime_evidence(report)
        and github_explore_target_url(report)
        and github_explore_safety_contract_ok(report)
        and github_explore_workspace_browser_ok(report)
        and github_explore_recommendations_ok(report)
    )


def audit_desktop_thin_integration(source_identity: dict[str, Any] | None) -> dict[str, Any]:
    if source_identity is None:
        return {
            "id": "desktop_thin_integration",
            "status": "passed",
            "summary": "Desktop integration shape was not checked because source identity checks are disabled.",
            "missing": [],
            "evidence": [],
        }
    if "components" not in source_identity:
        return {
            "id": "desktop_thin_integration",
            "status": "passed",
            "summary": "Desktop integration shape was not checked because this source identity has no Desktop component.",
            "missing": [],
            "evidence": [],
        }

    desktop_component = (source_identity.get("components") or {}).get("codex_desktop") or {}
    desktop_path = Path(str(desktop_component.get("path") or DEFAULT_DESKTOP_REPO))
    feature_dir = desktop_path / "linux-features" / "agent-workspace"
    root_generated_asset = desktop_path / "agent-workspaces-linux.js"
    patch_path = feature_dir / "patch.js"
    test_path = feature_dir / "test.js"
    patch_source = patch_path.read_text(encoding="utf-8") if patch_path.exists() else ""
    test_source = test_path.read_text(encoding="utf-8") if test_path.exists() else ""
    forbidden_patch_needles = [
        'id: "conversation-view"',
        "id: 'conversation-view'",
        'id:"conversation-view"',
        "id:'conversation-view'",
        "applyAgentWorkspaceConversationViewPatch",
        "agentWorkspaceConversationRuntimeSource",
        "CONVERSATION_RUNTIME_VERSION",
        "codexLinuxAgentWorkspaceConversationCleanup=cleanup",
        "codexLinuxAgentWorkspaceConversationVersion",
        "codex-linux-agent-workspace-panel",
    ]
    forbidden_bridge_needles = [
        "case\\`workspaceObserve\\`",
        "__codexAttachScreenshot",
        "data:image/png;base64",
    ]
    stale_runtime_cleanup_needles = [
        "stale-runtime-cleanup",
        "stripStaleAgentWorkspaceConversationRuntime",
    ]
    missing = []
    if desktop_component.get("exists") is not True:
        missing.append("sibling Codex Desktop agent-workspace feature source")
    if not feature_dir.exists():
        missing.append("Codex Desktop linux-features/agent-workspace directory")
    if root_generated_asset.exists():
        missing.append("remove stale root agent-workspaces-linux.js generated embedded-screen asset")
    if not patch_path.exists():
        missing.append("Codex Desktop agent-workspace patch.js")
    elif any(needle in patch_source for needle in forbidden_patch_needles):
        missing.append("Codex Desktop conversation embedded-screen runtime must remain removed")
    elif any(needle in patch_source for needle in forbidden_bridge_needles):
        missing.append("Codex Desktop main bridge generator must not carry embedded observe/screenshot code")
    elif not any(needle in patch_source for needle in stale_runtime_cleanup_needles):
        missing.append("Codex Desktop patcher must keep cleanup-only removal for stale conversation monitor runtime")
    if (
        "workspaceObserve" not in test_source
        or "doesNotMatch(patched, /case`workspaceObserve`/" not in test_source
    ):
        missing.append("Codex Desktop test coverage proving workspaceObserve is not exposed")

    return {
        "id": "desktop_thin_integration",
        "status": "passed" if not missing else "pending",
        "summary": (
            "Codex Desktop remains a thin settings/viewer launcher with only cleanup for stale embedded screen bundles."
            if not missing
            else "Codex Desktop integration may still contain or reintroduce the embedded screen surface."
        ),
        "missing": missing,
        "evidence": [
            {
                "desktop_repo": str(desktop_path),
                "desktop_source_identity": desktop_component,
                "root_generated_asset_exists": root_generated_asset.exists(),
                "patch_path": str(patch_path),
                "test_path": str(test_path),
                "conversation_view_needles_present": [
                    needle for needle in forbidden_patch_needles if needle in patch_source
                ],
                "embedded_bridge_needles_present": [
                    needle for needle in forbidden_bridge_needles if needle in patch_source
                ],
                "stale_runtime_cleanup_present": any(
                    needle in patch_source for needle in stale_runtime_cleanup_needles
                ),
                "workspace_observe_removed_test_present": (
                    "workspaceObserve" in test_source
                    and "doesNotMatch(patched, /case`workspaceObserve`/" in test_source
                ),
            }
        ],
    }


def passed_viewer_reports(
    viewer_dir: Path,
    *,
    now: dt.datetime,
    max_evidence_age_days: int,
    source_identity: dict[str, Any] | None,
) -> list[tuple[Path, dict[str, Any]]]:
    reports = load_json_reports(viewer_dir)
    passed: list[tuple[Path, dict[str, Any]]] = []
    for path, report in reports:
        if report.get("schema") != "agent-workspace-linux.viewer_desktop_matrix.v1":
            continue
        smoke = report.get("viewer_smoke") or {}
        matrix = report.get("matrix_result") or {}
        if (
            smoke.get("status") == "passed"
            and matrix.get("counts_for_release_matrix") is True
            and repo_owned_runtime_evidence(report)
            and viewer_matrix_session_release_eligible(report)
            and is_fresh_evidence(
                report, now=now, max_evidence_age_days=max_evidence_age_days
            )
            and source_identity_matches(report, source_identity)
        ):
            passed.append((path, report))
    return passed


def audit_viewer_matrix(
    viewer_dir: Path,
    *,
    now: dt.datetime,
    max_evidence_age_days: int,
    source_identity: dict[str, Any] | None,
) -> dict[str, Any]:
    passed = passed_viewer_reports(
        viewer_dir,
        now=now,
        max_evidence_age_days=max_evidence_age_days,
        source_identity=source_identity,
    )
    desktops = {desktop_text(report) for _, report in passed}
    sessions = {session_type(report) for _, report in passed}
    has_gnome = any("gnome" in desktop for desktop in desktops)
    has_kde = any("kde" in desktop or "plasma" in desktop for desktop in desktops)
    has_linux_desktop = bool(desktops)
    has_x11 = "x11" in sessions or any(
        x11_xwayland_viewer_protocol_observed(report) for _, report in passed
    )
    has_wayland = "wayland" in sessions
    native_wayland = any(native_wayland_observed(report) for _, report in passed)
    missing = []
    if not passed and max_evidence_age_days > 0:
        missing.append(f"fresh viewer matrix evidence within {max_evidence_age_days} days")
    if not passed and source_identity is not None:
        missing.append(
            f"viewer matrix evidence for current source hash {source_identity.get('source_hash')}"
        )
    if not passed:
        missing.append("viewer matrix evidence collected by the repo-owned runtime collector")

    if not has_linux_desktop:
        missing.append("Linux desktop viewer smoke row")
    if not has_x11:
        missing.append("X11/Xwayland viewer protocol evidence")
    if not has_wayland:
        missing.append("Wayland-like viewer smoke row")

    advisory_missing = []
    if not has_gnome:
        advisory_missing.append("GNOME viewer smoke row")
    if not has_kde:
        advisory_missing.append("KDE/Plasma viewer smoke row")
    if not native_wayland:
        advisory_missing.append("native Wayland layer-shell/compositor observation with notes")

    return {
        "id": "viewer_desktop_matrix",
        "status": "passed" if not missing else "pending",
        "summary": (
            "Viewer matrix covers the release-required Linux viewer surface."
            if not missing
            else "Viewer matrix still lacks release-required Linux desktop evidence."
        ),
        "missing": missing,
        "advisory_missing": advisory_missing,
        "evidence": [
            {
                "path": str(path),
                "source_identity": report.get("source_identity"),
                "desktop_label": (report.get("matrix_result") or {}).get("desktop_label"),
                "session_type": session_type(report) or None,
                "created_at_utc": report.get("created_at_utc"),
                "session_consistency": (report.get("matrix_result") or {}).get(
                    "session_consistency"
                ),
                "native_wayland_layer_shell_observed": native_wayland_observed(report),
                "x11_xwayland_viewer_protocol_observed": x11_xwayland_viewer_protocol_observed(
                    report
                ),
                "native_wayland_layer_shell_notes": (report.get("matrix_result") or {}).get(
                    "native_wayland_layer_shell_notes"
                ),
            }
            for path, report in passed
        ],
    }


def app_qa_dogfood_contract_ok(report: dict[str, Any]) -> bool:
    inputs = report.get("inputs") or {}
    safety = report.get("safety_contract") or {}
    workspace = report.get("workspace") or {}
    return (
        report.get("schema") == "agent-workspace-linux.app_qa_dogfood.v1"
        and repo_owned_runtime_evidence(report)
        and report.get("status") == "passed"
        and report.get("mode") == "local-gui-app"
        and inputs.get("task_intent") == "app_qa"
        and inputs.get("real_world_action_approved") is False
        and safety.get("hidden_workspace_acknowledged") is True
        and safety.get("app_qa_only") is True
        and safety.get("host_desktop_input_targeted") is False
        and safety.get("real_world_or_account_mutation") is False
        and safety.get("non_destructive_input_only") is True
        and workspace.get("status") == "passed"
        and workspace.get("launch_ok") is True
        and int(workspace.get("launch_window_count") or 0) >= 1
        and int(workspace.get("launch_screenshot_bytes") or 0) > 0
        and int(workspace.get("observe_screenshot_bytes") or 0) > 0
        and "app qa dogfood" in str(workspace.get("active_window_title") or "").lower()
        and int(workspace.get("event_count") or 0) > 0
        and workspace.get("logs_ok") is True
        and workspace.get("event_log_artifact_present") is True
        and workspace.get("stopped_by_workspace_stop") is True
        and workspace.get("stop_ok") is True
    )


def passed_app_qa_reports(
    app_qa_dir: Path,
    *,
    now: dt.datetime,
    max_evidence_age_days: int,
    source_identity: dict[str, Any] | None,
) -> list[tuple[Path, dict[str, Any]]]:
    reports = load_json_reports(app_qa_dir)
    passed: list[tuple[Path, dict[str, Any]]] = []
    for path, report in reports:
        if (
            app_qa_dogfood_contract_ok(report)
            and is_fresh_evidence(
                report, now=now, max_evidence_age_days=max_evidence_age_days
            )
            and source_identity_matches(report, source_identity)
        ):
            passed.append((path, report))
    return passed


def audit_app_qa_dogfood(
    app_qa_dir: Path,
    *,
    now: dt.datetime,
    max_evidence_age_days: int,
    source_identity: dict[str, Any] | None,
) -> dict[str, Any]:
    passed = passed_app_qa_reports(
        app_qa_dir,
        now=now,
        max_evidence_age_days=max_evidence_age_days,
        source_identity=source_identity,
    )
    missing = []
    if not passed:
        missing.append(
            "local GUI app-QA dogfood report with hidden workspace, screenshot, logs, events, and clean stop"
        )
        missing.append("app-QA dogfood evidence must use task_intent=app_qa and local-gui-app mode")
        missing.append(
            "app-QA dogfood evidence must prove non-destructive input only and no host desktop or real-world mutation"
        )
        missing.append("app-QA dogfood evidence collected by the repo-owned runtime collector")
        if max_evidence_age_days > 0:
            missing.append(f"fresh app-QA dogfood evidence within {max_evidence_age_days} days")
        if source_identity is not None:
            missing.append(
                f"app-QA dogfood evidence for current source hash {source_identity.get('source_hash')}"
            )
    return {
        "id": "app_qa_dogfood",
        "status": "passed" if not missing else "pending",
        "summary": (
            "Local GUI app-QA dogfood evidence proves observe/screenshot/log/event/stop flow."
            if not missing
            else "Local GUI app-QA dogfood evidence is missing or stale."
        ),
        "missing": missing,
        "evidence": [
            {
                "path": str(path),
                "source_identity": report.get("source_identity"),
                "created_at_utc": report.get("created_at_utc"),
                "mode": report.get("mode"),
                "target_app": (report.get("inputs") or {}).get("target_app"),
                "safety_contract": report.get("safety_contract"),
                "workspace": report.get("workspace"),
            }
            for path, report in passed
        ],
    }


def passed_real_grocery_reports(
    grocery_dir: Path,
    *,
    now: dt.datetime,
    max_evidence_age_days: int,
    source_identity: dict[str, Any] | None,
) -> list[tuple[Path, dict[str, Any]]]:
    reports = load_json_reports(grocery_dir)
    passed: list[tuple[Path, dict[str, Any]]] = []
    for path, report in reports:
        if report.get("schema") != "agent-workspace-linux.real_grocery_dogfood_probe.v1":
            continue
        inputs = report.get("inputs") or {}
        real_browser = report.get("real_browser") or {}
        if (
            report.get("mode") == "real-browser"
            and real_browser.get("status") == "passed"
            and real_browser.get("checkout_approval_refused") is True
            and real_browser.get("profile_copy_manifest_valid") is True
            and inputs.get("checkout_or_real_world_approved_env") is False
            and inputs.get("profile_is_disposable_copy_env") is True
            and real_grocery_target_url(report)
            and real_grocery_safety_contract_ok(report)
            and real_grocery_plan_assertions_ok(report)
            and real_grocery_workspace_browser_control_ok(report)
            and real_grocery_cart_draft_steps_manifest_ok(report)
            and real_grocery_cart_draft_interaction_ok(report)
            and real_grocery_workspace_input_audit_ok(report)
            and real_grocery_profile_directory_ok(report)
            and real_grocery_workspace_cleanup_ok(report)
            and repo_owned_runtime_evidence(report)
            and is_fresh_evidence(
                report, now=now, max_evidence_age_days=max_evidence_age_days
            )
            and source_identity_matches(report, source_identity)
        ):
            passed.append((path, report))
    return passed


def audit_real_grocery(
    grocery_dir: Path,
    *,
    now: dt.datetime,
    max_evidence_age_days: int,
    source_identity: dict[str, Any] | None,
) -> dict[str, Any]:
    passed = passed_real_grocery_reports(
        grocery_dir,
        now=now,
        max_evidence_age_days=max_evidence_age_days,
        source_identity=source_identity,
    )
    missing = []
    if not passed:
        missing.append(
            "real-browser grocery report with manifest-backed disposable copied profile, cart-draft approval, and checkout approval refused"
        )
        missing.append("real-browser grocery target URL must be an HTTPS non-local grocery site")
        missing.append(
            "real-browser grocery report must include passed plan assertions and cart-draft safety contract"
        )
        missing.append(
            "real-browser grocery report must prove workspace-owned browser target discovery and page snapshot through loopback DevTools"
        )
        missing.append(
            "real-browser grocery release evidence must omit raw logged-in page text"
        )
        missing.append(
            "real-browser grocery report must prove only declared cart-draft workspace input events"
        )
        missing.append(
            "real-browser grocery report must have consistent safe Chrome profile directory evidence when a profile directory is requested"
        )
        missing.append(
            "real-browser grocery report must prove the stopped workspace runtime was cleaned up"
        )
        missing.append(
            "real-browser grocery report must include a passed cart-draft interaction with at least one cart mutation step"
        )
        missing.append(
            "real-browser grocery report must bind the executed cart-draft interaction to the approved step-file hash"
        )
        missing.append(
            "real-browser grocery evidence must be collected by the repo-owned runtime collector"
        )
    if not passed and source_identity is not None:
        missing.append(
            f"real-browser grocery evidence for current source hash {source_identity.get('source_hash')}"
        )

    return {
        "id": "real_grocery_dogfood",
        "status": "passed" if not missing else "pending",
        "summary": (
            "Real logged-in grocery dogfood evidence exists without checkout/account approval."
            if not missing
            else "Real logged-in grocery dogfood is still unproven."
        ),
        "missing": missing,
        "evidence": [
            {
                "path": str(path),
                "source_identity": report.get("source_identity"),
                "created_at_utc": report.get("created_at_utc"),
                "target_url": (report.get("inputs") or {}).get("target_url"),
                "workspace_id": (report.get("real_browser") or {}).get("workspace_id"),
                "checkout_approval_refused": (report.get("real_browser") or {}).get(
                    "checkout_approval_refused"
                ),
                "profile_copy_manifest": (
                    (report.get("real_browser") or {}).get("profile_copy_manifest") or {}
                ).get("path"),
                "profile_directory": (report.get("real_browser") or {}).get(
                    "profile_directory"
                ),
                "chrome_devtools": (report.get("real_browser") or {}).get(
                    "chrome_devtools"
                ),
                "safety_contract": report.get("safety_contract"),
                "plan_assertions": report.get("plan_assertions"),
                "cart_draft_interaction": (report.get("real_browser") or {}).get(
                    "cart_draft_interaction"
                ),
                "cart_draft_steps": (report.get("real_browser") or {}).get(
                    "cart_draft_steps"
                ),
                "workspace_input_audit": (report.get("real_browser") or {}).get(
                    "workspace_input_audit"
                ),
            }
            for path, report in passed
        ],
    }


def passed_github_explore_reports(
    github_explore_dir: Path,
    *,
    now: dt.datetime,
    max_evidence_age_days: int,
    source_identity: dict[str, Any] | None,
) -> list[tuple[Path, dict[str, Any]]]:
    reports = load_json_reports(github_explore_dir)
    passed: list[tuple[Path, dict[str, Any]]] = []
    for path, report in reports:
        if (
            github_explore_dogfood_contract_ok(report)
            and is_fresh_evidence(
                report, now=now, max_evidence_age_days=max_evidence_age_days
            )
            and source_identity_matches(report, source_identity)
        ):
            passed.append((path, report))
    return passed


def audit_github_explore_dogfood(
    github_explore_dir: Path,
    *,
    now: dt.datetime,
    max_evidence_age_days: int,
    source_identity: dict[str, Any] | None,
) -> dict[str, Any]:
    passed = passed_github_explore_reports(
        github_explore_dir,
        now=now,
        max_evidence_age_days=max_evidence_age_days,
        source_identity=source_identity,
    )
    missing = []
    if not passed:
        missing.append(
            "GitHub Explore dogfood report with at least three repository recommendations"
        )
        missing.append("GitHub Explore target URL must be a public HTTPS github.com explore page")
        missing.append(
            "GitHub Explore dogfood must use workspace-owned Chrome DevTools without host Chrome bridge, Playwright, curl, or coordinate input"
        )
        missing.append(
            "GitHub Explore dogfood must open the GPUI viewer through workspace_open_viewer and include launch metadata"
        )
        missing.append(
            "GitHub Explore dogfood must prove screenshot, event, DevTools, and clean workspace stop evidence"
        )
        missing.append(
            "GitHub Explore dogfood evidence must be collected by the repo-owned runtime collector"
        )
        if max_evidence_age_days > 0:
            missing.append(
                f"fresh GitHub Explore dogfood evidence within {max_evidence_age_days} days"
            )
        if source_identity is not None:
            missing.append(
                f"GitHub Explore dogfood evidence for current source hash {source_identity.get('source_hash')}"
            )

    return {
        "id": "github_explore_dogfood",
        "status": "passed" if not missing else "pending",
        "summary": (
            "GitHub Explore dogfood proves visible workspace browser repository discovery."
            if not missing
            else "GitHub Explore repository-discovery dogfood is missing or stale."
        ),
        "missing": missing,
        "evidence": [
            {
                "path": str(path),
                "source_identity": report.get("source_identity"),
                "created_at_utc": report.get("created_at_utc"),
                "target_url": (report.get("inputs") or {}).get("target_url"),
                "workspace_browser": report.get("workspace_browser"),
                "recommendations": report.get("recommendations"),
                "safety_contract": report.get("safety_contract"),
            }
            for path, report in passed
        ],
    }


def audit_human_review(
    marker_path: Path,
    *,
    now: dt.datetime,
    max_evidence_age_days: int,
    source_identity: dict[str, Any] | None,
    review_scope_identity: dict[str, Any] | None,
) -> dict[str, Any]:
    marker = None
    if marker_path.exists():
        try:
            marker = json.loads(marker_path.read_text(encoding="utf-8"))
        except Exception:
            marker = None
    if (
        isinstance(marker, dict)
        and marker.get("schema") == "agent-workspace-linux.human_final_diff_review.v1"
        and marker.get("status") == "reviewed"
        and is_fresh_evidence(marker, now=now, max_evidence_age_days=max_evidence_age_days)
        and source_identity_matches(marker, source_identity)
        and review_scope_matches(marker, review_scope_identity)
        and review_artifacts_match(marker)
        and human_review_metadata_ok(marker)
    ):
        return {
            "id": "human_final_diff_review",
            "status": "passed",
            "summary": "Final human diff review marker is present.",
            "missing": [],
            "evidence": [
                {
                    "path": str(marker_path),
                    "source_identity": marker.get("source_identity"),
                    "review_scope_identity": marker.get("review_scope_identity"),
                    "reviewed_at_utc": marker.get("reviewed_at_utc"),
                    "reviewer": marker.get("reviewer"),
                    "notes": marker.get("notes"),
                    "review_artifacts": marker.get("review_artifacts"),
                }
            ],
        }

    return {
        "id": "human_final_diff_review",
        "status": "pending",
        "summary": "Final human diff review cannot be inferred from local tests.",
        "missing": [
            "human review of runtime and sibling Desktop diffs before staging/shipping",
            f"marker file with schema agent-workspace-linux.human_final_diff_review.v1 at {marker_path}",
            f"human review marker reviewed_at_utc within {max_evidence_age_days} days",
            (
                f"human review marker for current source hash {source_identity.get('source_hash')}"
                if source_identity is not None
                else "human review marker source identity"
            ),
            (
                "human review marker for current runtime/Desktop review scope "
                f"{review_scope_identity.get('review_scope_hash')}"
                if review_scope_identity is not None
                else "human review marker review scope identity"
            ),
            "human review marker reviewer and notes must be meaningful and not placeholders",
            "human review marker review_artifacts for runtime and sibling Desktop review diffs with matching sha256",
        ],
        "evidence": [],
    }


def audit_source_clean(source_identity: dict[str, Any] | None) -> dict[str, Any]:
    dirty_count = None if source_identity is None else source_identity.get("source_dirty_count")
    missing_components = (
        []
        if source_identity is None
        else list(source_identity.get("missing_components") or [])
    )
    if dirty_count == 0 and not missing_components:
        return {
            "id": "source_clean",
            "status": "passed",
            "summary": "Runtime and Desktop source identity is clean.",
            "missing": [],
            "evidence": [{"source_identity": source_identity}],
        }
    return {
        "id": "source_clean",
        "status": "pending",
        "summary": "Runtime and Desktop source identity is dirty, incomplete, or unavailable.",
        "missing": [
            "clean runtime source tree for Cargo.toml, Cargo.lock, src/, and scripts/",
            "clean sibling Codex Desktop feature tree for linux-features/agent-workspace and no stale root agent-workspaces-linux.js",
        ],
        "evidence": [{"source_identity": source_identity}],
    }


def build_report(
    *,
    viewer_dir: Path,
    app_qa_dir: Path,
    grocery_dir: Path,
    github_explore_dir: Path,
    human_review_marker: Path,
    now: dt.datetime,
    max_evidence_age_days: int,
    source_identity: dict[str, Any] | None,
    require_clean_source: bool,
    review_scope_identity: dict[str, Any] | None = None,
    include_legacy_grocery: bool = False,
    include_github_explore: bool = True,
) -> dict[str, Any]:
    gates = [
        audit_desktop_thin_integration(source_identity),
        audit_viewer_matrix(
            viewer_dir,
            now=now,
            max_evidence_age_days=max_evidence_age_days,
            source_identity=source_identity,
        ),
        audit_app_qa_dogfood(
            app_qa_dir,
            now=now,
            max_evidence_age_days=max_evidence_age_days,
            source_identity=source_identity,
        ),
        audit_human_review(
            human_review_marker,
            now=now,
            max_evidence_age_days=max_evidence_age_days,
            source_identity=source_identity,
            review_scope_identity=review_scope_identity,
        ),
    ]
    if include_github_explore:
        gates.insert(
            3,
            audit_github_explore_dogfood(
                github_explore_dir,
                now=now,
                max_evidence_age_days=max_evidence_age_days,
                source_identity=source_identity,
            ),
        )
    if include_legacy_grocery:
        gates.insert(
            4 if include_github_explore else 3,
            audit_real_grocery(
                grocery_dir,
                now=now,
                max_evidence_age_days=max_evidence_age_days,
                source_identity=source_identity,
            ),
        )
    if require_clean_source:
        gates.append(audit_source_clean(source_identity))
    status = "passed" if all(gate["status"] == "passed" for gate in gates) else "pending"
    return {
        "schema": "agent-workspace-linux.release_gate_audit.v1",
        "created_at_utc": now.isoformat(),
        "repo": str(ROOT),
        "inputs": {
            "viewer_dir": str(viewer_dir),
            "app_qa_dir": str(app_qa_dir),
            "github_explore_dir": str(github_explore_dir),
            "legacy_grocery_dir": str(grocery_dir),
            "include_legacy_grocery": include_legacy_grocery,
            "human_review_marker": str(human_review_marker),
            "max_evidence_age_days": max_evidence_age_days,
            "desktop_repo": (
                (((source_identity or {}).get("components") or {}).get("codex_desktop") or {}).get(
                    "path"
                )
            ),
            "source_identity": source_identity,
            "review_scope_identity": review_scope_identity,
            "require_clean_source": require_clean_source,
        },
        "status": status,
        "gates": gates,
    }


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def assert_self_test(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(f"release gate audit self-test failed: {message}")


def run_review_scope_self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="agent-workspace-review-scope-") as temp:
        repo = Path(temp)
        subprocess.run(["git", "init"], cwd=repo, check=True, capture_output=True)
        subprocess.run(
            ["git", "config", "user.email", "review-scope@example.invalid"],
            cwd=repo,
            check=True,
        )
        subprocess.run(
            ["git", "config", "user.name", "Review Scope"],
            cwd=repo,
            check=True,
        )
        (repo / "tracked.txt").write_text("one\n", encoding="utf-8")
        subprocess.run(["git", "add", "tracked.txt"], cwd=repo, check=True)
        subprocess.run(["git", "commit", "-m", "one"], cwd=repo, check=True, capture_output=True)
        clean_one = compute_repo_review_scope_identity(repo, label="runtime")
        assert_self_test(
            clean_one["review_mode"] == "clean_head",
            "clean review scope should bind to HEAD commit content",
        )

        (repo / "tracked.txt").write_text("two\n", encoding="utf-8")
        dirty = compute_repo_review_scope_identity(repo, label="runtime")
        assert_self_test(
            dirty["review_mode"] == "dirty_worktree",
            "dirty review scope should bind to worktree diff",
        )
        assert_self_test(
            dirty["review_scope_hash"] != clean_one["review_scope_hash"],
            "dirty diff should change review scope hash",
        )

        subprocess.run(["git", "add", "tracked.txt"], cwd=repo, check=True)
        subprocess.run(["git", "commit", "-m", "two"], cwd=repo, check=True, capture_output=True)
        clean_two = compute_repo_review_scope_identity(repo, label="runtime")
        assert_self_test(
            clean_two["review_mode"] == "clean_head",
            "clean review scope should return to clean_head after commit",
        )
        assert_self_test(
            clean_two["review_scope_hash"] != clean_one["review_scope_hash"],
            "new HEAD commit should change clean review scope hash",
        )


def run_self_test() -> None:
    run_review_scope_self_test()
    with tempfile.TemporaryDirectory(prefix="agent-workspace-release-audit-") as temp:
        root = Path(temp)
        viewer_dir = root / "viewer"
        app_qa_dir = root / "app-qa"
        grocery_dir = root / "grocery"
        github_explore_dir = root / "github-explore"
        marker = root / "human-review.json"
        now = dt.datetime.now(dt.timezone.utc)
        fresh_stamp = now.isoformat()
        stale_stamp = (now - dt.timedelta(days=DEFAULT_MAX_EVIDENCE_AGE_DAYS + 1)).isoformat()
        source_identity = {
            "git_head": "release-gate-self-test-head",
            "source_hash": "release-gate-self-test-source",
            "source_paths": SOURCE_IDENTITY_PATHS,
        }
        evidence_boundary = {
            "collector": "agent-workspace-linux",
            "collector_script": "self-test",
            "repo_owned_runtime": True,
            "codex_app_mcp_used": False,
            "computer_use_mcp_used": False,
            "codex_desktop_bridge_used": False,
            "playwright_mcp_used": False,
        }
        old_boundary_without_playwright = dict(evidence_boundary)
        old_boundary_without_playwright.pop("playwright_mcp_used")
        assert_self_test(
            not repo_owned_runtime_evidence(
                {"evidence_boundary": old_boundary_without_playwright}
            ),
            "repo-owned evidence must explicitly reject missing Playwright MCP boundary",
        )
        review_scope_identity = {
            "git_head": "release-gate-self-test-review-head",
            "review_scope_hash": "release-gate-self-test-review-scope",
            "dirty_count": 4,
        }
        github_explore_report = {
            "schema": "agent-workspace-linux.github_explore_dogfood.v1",
            "created_at_utc": fresh_stamp,
            "source_identity": source_identity,
            "evidence_boundary": evidence_boundary,
            "mode": "workspace-github-explore",
            "status": "passed",
            "inputs": {
                "task_intent": "github_explore_repository_discovery",
                "target_url": "https://github.com/explore",
            },
            "safety_contract": {
                "public_repository_discovery_only": True,
                "no_host_browser_bridge": True,
                "no_playwright_or_curl": True,
                "no_account_mutation": True,
                "raw_page_text_omitted_from_report": True,
            },
            "workspace_browser": {
                "status": "passed",
                "control_surface": "direct_mcp_workspace_browser_devtools",
                "workspace_owned_browser": True,
                "host_chrome_bridge_used": False,
                "coordinate_input_used": False,
                "workspace_id": "github-explore-self-test",
                "browser_app_id": "app-github-explore",
                "page_url": "https://github.com/explore",
                "page_title": "Explore GitHub",
                "target_id": "page-github-explore",
                "target_count": 1,
                "devtools_endpoint": "http://127.0.0.1:45555",
                "snapshot_text_chars": 4096,
                "launch_screenshot_bytes": 8192,
                "event_count": 4,
                "viewer": {
                    "ok": True,
                    "launch": {
                        "id": "github-explore-self-test",
                        "pid": 12345,
                        "backend": "x11-popup-topmost",
                        "always_on_top": True,
                        "exit_when_workspace_gone": True,
                        "command": ["agent-workspace-linux", "viewer"],
                    },
                },
                "cleanup": {"ok": True},
            },
            "recommendations": [
                {
                    "full_name": "Lum1104/Understand-Anything",
                    "url": "https://github.com/Lum1104/Understand-Anything",
                    "reason": "Codebase-understanding and Codex-adjacent workflow work.",
                    "matched_terms": ["codex", "knowledge-graph"],
                },
                {
                    "full_name": "rohitg00/ai-engineering-from-scratch",
                    "url": "https://github.com/rohitg00/ai-engineering-from-scratch",
                    "reason": "Low-level AI engineering, MCP, and agents study.",
                    "matched_terms": ["rust", "mcp"],
                },
                {
                    "full_name": "colbymchenry/codegraph",
                    "url": "https://github.com/colbymchenry/codegraph",
                    "reason": "Local code graph for fewer tokens and tool calls.",
                    "matched_terms": ["codex", "local"],
                },
            ],
            "recommendation_count": 3,
        }
        write_json(github_explore_dir / "github-explore.json", github_explore_report)
        assert_self_test(
            github_explore_dogfood_contract_ok(github_explore_report),
            "GitHub Explore dogfood fixture should satisfy the release contract",
        )
        github_without_viewer = json.loads(json.dumps(github_explore_report))
        github_without_viewer["workspace_browser"]["viewer"] = {
            "ok": True,
            "launch": None,
        }
        assert_self_test(
            not github_explore_dogfood_contract_ok(github_without_viewer),
            "GitHub Explore dogfood must reject reports without workspace_open_viewer launch metadata",
        )
        original_build_report = globals()["build_report"]

        def build_report(**kwargs: Any) -> dict[str, Any]:
            return original_build_report(
                **kwargs,
                github_explore_dir=github_explore_dir,
                include_legacy_grocery=True,
                include_github_explore=False,
            )

        desktop_repo = root / "codex-desktop-linux"
        feature_dir = desktop_repo / "linux-features" / "agent-workspace"
        feature_dir.mkdir(parents=True)
        (feature_dir / "patch.js").write_text(
            'function stripStaleAgentWorkspaceConversationRuntime(){};'
            'module.exports={patches:[{id:"main-bridge"},{id:"settings-page"},{id:"stale-runtime-cleanup"},{id:"approval-rendering"}]};\n',
            encoding="utf-8",
        )
        (feature_dir / "test.js").write_text(
            "assert.doesNotMatch(patched, /case`workspaceObserve`/);\n",
            encoding="utf-8",
        )
        desktop_source_identity = {
            "git_head": "release-gate-self-test-desktop-head",
            "source_hash": "release-gate-self-test-desktop-source",
            "components": {
                "codex_desktop": {
                    "exists": True,
                    "path": str(desktop_repo),
                    "source_hash": "release-gate-self-test-desktop-source",
                }
            },
        }
        thin_gate = audit_desktop_thin_integration(desktop_source_identity)
        assert_self_test(
            thin_gate["status"] == "passed",
            "Desktop thin integration should pass with only the cleanup patch for stale conversation bundles",
        )
        (desktop_repo / "agent-workspaces-linux.js").write_text(
            "stale embedded screen asset\n",
            encoding="utf-8",
        )
        stale_desktop_gate = audit_desktop_thin_integration(desktop_source_identity)
        assert_self_test(
            "remove stale root agent-workspaces-linux.js generated embedded-screen asset"
            in stale_desktop_gate["missing"],
            "Desktop thin integration should reject the stale root generated asset",
        )
        (desktop_repo / "agent-workspaces-linux.js").unlink()
        (feature_dir / "patch.js").write_text(
            'function applyAgentWorkspaceConversationViewPatch(){};'
            'module.exports={patches:[{id:"conversation-view"}]};\n',
            encoding="utf-8",
        )
        revived_desktop_gate = audit_desktop_thin_integration(desktop_source_identity)
        assert_self_test(
            "Codex Desktop conversation embedded-screen runtime must remain removed"
            in revived_desktop_gate["missing"],
            "Desktop thin integration should reject revived conversation-view patches",
        )
        (feature_dir / "patch.js").write_text(
            'function stripStaleAgentWorkspaceConversationRuntime(){};'
            'module.exports={patches:[{id:"stale-runtime-cleanup"}]};\n',
            encoding="utf-8",
        )
        cleanup_desktop_gate = audit_desktop_thin_integration(desktop_source_identity)
        assert_self_test(
            cleanup_desktop_gate["status"] == "passed",
            "Desktop thin integration should allow cleanup-only stale runtime removal patches",
        )
        (feature_dir / "patch.js").write_text(
            'module.exports={patches:[{id:"main-bridge"}]};\n',
            encoding="utf-8",
        )
        missing_cleanup_desktop_gate = audit_desktop_thin_integration(desktop_source_identity)
        assert_self_test(
            "Codex Desktop patcher must keep cleanup-only removal for stale conversation monitor runtime"
            in missing_cleanup_desktop_gate["missing"],
            "Desktop thin integration should require the stale conversation cleanup patch",
        )
        (feature_dir / "patch.js").write_text(
            'module.exports={patches:[{id:"main-bridge"}]};'
            "const source='case\\`workspaceObserve\\`:__codexAttachScreenshot data:image/png;base64';\n",
            encoding="utf-8",
        )
        embedded_bridge_desktop_gate = audit_desktop_thin_integration(desktop_source_identity)
        assert_self_test(
            "Codex Desktop main bridge generator must not carry embedded observe/screenshot code"
            in embedded_bridge_desktop_gate["missing"],
            "Desktop thin integration should reject embedded observe bridge code in the generator",
        )
        (feature_dir / "patch.js").write_text(
            'module.exports={patches:[{id:"main-bridge"},{id:"settings-page"},{id:"approval-rendering"}]};\n',
            encoding="utf-8",
        )
        runtime_root = root / "agent-workspace-linux"
        (runtime_root / "src").mkdir(parents=True)
        (runtime_root / "scripts").mkdir()
        (runtime_root / "Cargo.toml").write_text(
            "[package]\nname = \"agent-workspace-linux-test\"\nversion = \"0.0.0\"\n",
            encoding="utf-8",
        )
        (runtime_root / "Cargo.lock").write_text("# lock\n", encoding="utf-8")
        (runtime_root / "src" / "main.rs").write_text("fn main() {}\n", encoding="utf-8")
        (runtime_root / "scripts" / "smoke.sh").write_text("#!/usr/bin/env bash\ntrue\n", encoding="utf-8")
        manifest = {
            "source_identity": compute_source_identity(runtime_root, desktop_repo=desktop_repo)
        }
        assert_self_test(
            bundle_source_content_errors(manifest, root=runtime_root, desktop_repo=desktop_repo)
            == [],
            "bundle source verifier should accept unchanged source bytes",
        )
        (runtime_root / "src" / "main.rs").write_text("fn main() { println!(\"changed\"); }\n", encoding="utf-8")
        mismatch_errors = bundle_source_content_errors(
            manifest,
            root=runtime_root,
            desktop_repo=desktop_repo,
        )
        assert_self_test(
            any("runtime source_hash" in error for error in mismatch_errors),
            "bundle source verifier should catch changed runtime source bytes",
        )
        try:
            validate_bundle_manifest_source_contents(
                manifest,
                root=runtime_root,
                desktop_repo=desktop_repo,
            )
        except RuntimeError as error:
            assert_self_test(
                "source bytes no longer match" in str(error),
                "bundle source verifier should explain mismatched source bytes",
            )
        else:
            raise AssertionError(
                "release gate audit self-test failed: mismatched bundle source should reject"
            )

        pending = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        assert_self_test(pending["status"] == "pending", "empty evidence must stay pending")

        release_display_attestation = {
            "release_eligible": True,
            "problems": [],
            "warnings": [],
            "display_protocols": ["x11"],
            "sockets": [
                {
                    "kind": "x11",
                    "path": "/tmp/.X11-unix/X0",
                    "exists": True,
                    "processes": [{"command": "Xorg", "pid": 100, "args": "/usr/lib/Xorg :0"}],
                }
            ],
            "known_nested_or_headless_processes": [],
            "lsof_available": True,
        }
        release_wayland_display_attestation = {
            **release_display_attestation,
            "display_protocols": ["wayland", "x11"],
            "sockets": [
                {
                    "kind": "wayland",
                    "path": "/run/user/1000/wayland-0",
                    "exists": True,
                    "processes": [
                        {
                            "command": "kwin_wayland",
                            "pid": 200,
                            "args": "kwin_wayland --wayland_fd 5",
                        }
                    ],
                },
                {
                    "kind": "x11",
                    "path": "/tmp/.X11-unix/X0",
                    "exists": True,
                    "processes": [
                        {
                            "command": "Xwayland",
                            "pid": 201,
                            "args": "Xwayland :0 -rootless -noreset",
                        }
                    ],
                },
            ],
        }
        x11_xwayland_smoke_summary = {
            "schema": "agent-workspace-linux.gpui_viewer_smoke_summary.v1",
            "viewer_backend_forced": "x11",
            "x11_xwayland_window_observed": True,
            "default_viewer": {
                "skip_taskbar": True,
                "skip_pager": True,
                "above": False,
                "sticky": False,
                "notification_or_utility": True,
            },
            "duplicate_launch": {
                "reused_existing_instance": True,
                "window_count_for_original_pid": 1,
            },
            "topmost_viewer": {
                "above": True,
                "sticky": True,
            },
            "target_bound_viewer_exited_after_workspace_cleanup": True,
        }
        cart_draft_steps_path = "/tmp/release-gate-self-test-cart-draft-steps.json"
        cart_draft_steps_sha256 = "0" * 64
        cart_draft_steps_size_bytes = 512
        cart_draft_step_summaries = [
            {
                "index": 1,
                "action": "key_window",
                "safety_label": "Focus the grocery site search field.",
                "cart_mutation": False,
                "text_bytes": None,
            },
            {
                "index": 2,
                "action": "paste_window",
                "safety_label": "Enter only the approved shopping-list text.",
                "cart_mutation": False,
                "text_bytes": 29,
            },
            {
                "index": 3,
                "action": "key_window",
                "safety_label": "Confirm the approved draft-cart action only.",
                "cart_mutation": True,
                "text_bytes": None,
            },
            {
                "index": 4,
                "action": "observe",
                "safety_label": "Collect evidence that the cart draft is visible.",
                "cart_mutation": False,
                "text_bytes": None,
            },
        ]

        write_json(
            viewer_dir / "gnome-x11.json",
            {
                "schema": "agent-workspace-linux.viewer_desktop_matrix.v1",
                "created_at_utc": stale_stamp,
                "source_identity": source_identity,
                "evidence_boundary": evidence_boundary,
                "session": {
                    "xdg_current_desktop": "GNOME",
                    "desktop_session": "gnome",
                    "xdg_session_type": "x11",
                },
                "viewer_smoke": {"status": "passed"},
                "matrix_result": {
                    "counts_for_release_matrix": True,
                    "desktop_label": "GNOME / x11",
                    "display_attestation": release_display_attestation,
                },
            },
        )
        write_json(
            viewer_dir / "kde-wayland.json",
            {
                "schema": "agent-workspace-linux.viewer_desktop_matrix.v1",
                "created_at_utc": stale_stamp,
                "source_identity": source_identity,
                "evidence_boundary": evidence_boundary,
                "session": {
                    "xdg_current_desktop": "KDE",
                    "desktop_session": "plasma",
                    "xdg_session_type": "wayland",
                },
                "viewer_smoke": {"status": "passed"},
                "matrix_result": {
                    "counts_for_release_matrix": True,
                    "desktop_label": "KDE / wayland",
                    "display_attestation": release_wayland_display_attestation,
                    "native_wayland_layer_shell_observed": True,
                    "native_wayland_layer_shell_notes": "Observed layer-shell placement and top-layer behavior on a native Wayland compositor.",
                },
            },
        )
        write_json(
            viewer_dir / "gnome-wayland-xwayland-negative.json",
            {
                "schema": "agent-workspace-linux.viewer_desktop_matrix.v1",
                "created_at_utc": stale_stamp,
                "source_identity": source_identity,
                "evidence_boundary": evidence_boundary,
                "session": {
                    "xdg_current_desktop": "GNOME",
                    "desktop_session": "gnome",
                    "xdg_session_type": "wayland",
                },
                "viewer_smoke": {"status": "passed", "summary": x11_xwayland_smoke_summary},
                "matrix_result": {
                    "counts_for_release_matrix": True,
                    "desktop_label": "GNOME / wayland",
                    "display_attestation": release_wayland_display_attestation,
                    "x11_xwayland_viewer_protocol_observed": True,
                    "native_wayland_layer_shell_observed": True,
                    "native_wayland_layer_shell_notes": "Observed a normal resizable Xwayland toplevel, not layer-shell/top-layer behavior.",
                },
            },
        )
        write_json(
            grocery_dir / "real-browser.json",
            {
                "schema": "agent-workspace-linux.real_grocery_dogfood_probe.v1",
                "created_at_utc": stale_stamp,
                "source_identity": source_identity,
                "evidence_boundary": evidence_boundary,
                "mode": "real-browser",
                "inputs": {
                    "target_url": "https://grocery-release-gate.example-retailer.com",
                    "profile_directory": "Profile 1",
                    "checkout_or_real_world_approved_env": False,
                    "profile_is_disposable_copy_env": True,
                    "cart_mutation_approved_env": True,
                    "final_cart_reviewed_env": True,
                    "real_browser_interaction_mode": "cart-draft-approved",
                    "cart_draft_steps_path": cart_draft_steps_path,
                },
                "safety_contract": {
                    "refuses_checkout_or_real_world_approval": True,
                    "real_browser_requires_disposable_profile_copy": True,
                    "real_browser_interaction_mode": "cart-draft-approved",
                    "cart_draft_requires_explicit_approval": True,
                    "checkout_order_or_account_change_blocked": True,
                    "real_browser_sends_no_workspace_input": False,
                    "real_browser_observes_only": False,
                    "real_browser_allows_only_declared_cart_draft_input": True,
                    "real_browser_cleans_workspace_runtime": True,
                },
                "plan_assertions": {
                    "status": "passed",
                    "unapproved_next_boundary": {"kind": "approval"},
                    "cart_only_next_boundary": {"kind": "approval"},
                    "checkout_still_blocked_after_cart_approval": True,
                },
                "real_browser": {
                    "status": "passed",
                    "interaction_mode": "cart-draft-approved",
                    "workspace_id": "release-gate-self-test",
                    "browser_app_id": "app-release-grocery",
                    "profile_directory": "Profile 1",
                    "checkout_approval_refused": True,
                    "profile_copy_manifest_valid": True,
                    "profile_copy_manifest": {
                        "path": "/tmp/profile-copy-manifest.json",
                        "profile_directory": "Profile 1",
                    },
                    "cart_draft_steps": {
                        "path": cart_draft_steps_path,
                        "sha256": cart_draft_steps_sha256,
                        "size_bytes": cart_draft_steps_size_bytes,
                        "step_count": 4,
                        "input_step_count": 3,
                        "cart_mutation_step_count": 1,
                        "summaries": cart_draft_step_summaries,
                    },
                    "cart_draft_interaction": {
                        "status": "passed",
                        "mode": "cart-draft-approved",
                        "steps_path": cart_draft_steps_path,
                        "steps_sha256": cart_draft_steps_sha256,
                        "steps_size_bytes": cart_draft_steps_size_bytes,
                        "step_count": 4,
                        "input_step_count": 3,
                        "cart_mutation_step_count": 1,
                        "forbidden_step_count": 0,
                        "cart_mutation_approval_confirmed": True,
                        "final_cart_reviewed_confirmed": True,
                        "checkout_or_real_world_approval_refused": True,
                        "executed_steps": [
                            {
                                "index": 1,
                                "action": "key_window",
                                "safety_label": "Focus the grocery site search field.",
                                "cart_mutation": False,
                                "text_bytes": None,
                                "result": {"ok": True},
                            },
                            {
                                "index": 2,
                                "action": "paste_window",
                                "safety_label": "Enter only the approved shopping-list text.",
                                "cart_mutation": False,
                                "text_bytes": 29,
                                "result": {"ok": True},
                            },
                            {
                                "index": 3,
                                "action": "key_window",
                                "safety_label": "Confirm the approved draft-cart action only.",
                                "cart_mutation": True,
                                "text_bytes": None,
                                "result": {"ok": True},
                            },
                            {
                                "index": 4,
                                "action": "observe",
                                "safety_label": "Collect evidence that the cart draft is visible.",
                                "cart_mutation": False,
                                "text_bytes": None,
                                "result": {"ok": True},
                            },
                        ],
                    },
                    "workspace_input_audit": {
                        "checked": True,
                        "event_scope": "since_sequence",
                        "events_since_sequence": 9,
                        "events_tail_requested": 120,
                        "minimum_events_tail_required": 120,
                        "total_events": 3,
                        "expected_input_step_count": 3,
                        "input_event_count": 3,
                        "input_event_count_covers_expected": True,
                        "input_event_kinds": ["key_window", "paste_window"],
                        "input_event_sequences": [10, 11, 12],
                        "allowed_input_event_kinds": [
                            "click_window",
                            "key_window",
                            "paste_window",
                            "scroll_window",
                            "type_window",
                        ],
                        "unexpected_input_event_count": 0,
                        "unexpected_input_event_kinds": [],
                        "unexpected_input_event_sequences": [],
                    },
                    "chrome_devtools": {
                        "status": "passed",
                        "control_surface": "workspace_chrome_devtools",
                        "workspace_owned_browser": True,
                        "host_chrome_bridge_used": False,
                        "coordinate_input_used": False,
                        "endpoint": "http://127.0.0.1:42222",
                        "target_count": 1,
                        "target": {
                            "id": "page-release-grocery",
                            "type": "page",
                            "title": "Release Grocery",
                            "url": "https://grocery-release-gate.example-retailer.com/cart",
                        },
                        "workspace_browser_targets": {
                            "ok": True,
                            "message": "workspace browser targets returned for app app-release-grocery through workspace-owned Chrome DevTools",
                            "app_id": "app-release-grocery",
                            "app_pid": 4242,
                            "workspace_user_data_dir": "/tmp/grocery-profile-copy",
                            "host_user_data_dir": "/tmp/grocery-profile-copy",
                            "devtools_endpoint": "http://127.0.0.1:42222",
                            "devtools_active_port_path": "/tmp/grocery-profile-copy/DevToolsActivePort",
                            "target_count": 1,
                            "selected_page_target": {
                                "id": "page-release-grocery",
                                "type": "page",
                                "title": "Release Grocery",
                                "url": "https://grocery-release-gate.example-retailer.com/cart",
                                "webSocketDebuggerUrl": "ws://127.0.0.1:42222/devtools/page/page-release-grocery",
                            },
                            "warnings": [],
                        },
                        "workspace_browser_snapshot": {
                            "ok": True,
                            "message": "workspace browser page snapshot captured through workspace-owned Chrome DevTools",
                            "app_id": "app-release-grocery",
                            "target_id": "page-release-grocery",
                            "page_url": "https://grocery-release-gate.example-retailer.com/cart",
                            "page_title": "Release Grocery",
                            "text_chars": 128,
                            "text_truncated": False,
                            "warnings": [],
                        },
                        "page_snapshot": {
                            "source": "workspace_browser_snapshot",
                            "title": "Release Grocery",
                            "url": "https://grocery-release-gate.example-retailer.com/cart",
                            "text_chars": 128,
                            "text_truncated": False,
                            "raw_text_omitted": True,
                            "raw_text_omission_reason": "release evidence avoids storing logged-in grocery page text",
                        },
                    },
                    "cleanup": {
                        "dry_run": False,
                        "removed": [{"id": "release-gate-self-test"}],
                        "skipped": [],
                    },
                },
            },
        )
        write_json(
            app_qa_dir / "local-gui.json",
            {
                "schema": "agent-workspace-linux.app_qa_dogfood.v1",
                "created_at_utc": stale_stamp,
                "source_identity": source_identity,
                "evidence_boundary": evidence_boundary,
                "mode": "local-gui-app",
                "status": "passed",
                "inputs": {
                    "task_intent": "app_qa",
                    "target_app": "xmessage",
                    "real_world_action_approved": False,
                },
                "safety_contract": {
                    "hidden_workspace_acknowledged": True,
                    "app_qa_only": True,
                    "host_desktop_input_targeted": False,
                    "real_world_or_account_mutation": False,
                    "non_destructive_input_only": True,
                },
                "workspace": {
                    "status": "passed",
                    "launch_ok": True,
                    "launch_window_count": 1,
                    "launch_screenshot_bytes": 2048,
                    "observe_screenshot_bytes": 4096,
                    "active_window_title": "App QA Dogfood Target",
                    "event_count": 5,
                    "logs_ok": True,
                    "event_log_artifact_present": True,
                    "stopped_by_workspace_stop": True,
                    "stop_ok": True,
                },
            },
        )
        runtime_review_artifact = root / "runtime-review.diff"
        desktop_review_artifact = root / "codex-desktop-review.diff"
        runtime_review_artifact.write_text("reviewed runtime diff\n", encoding="utf-8")
        desktop_review_artifact.write_text("reviewed desktop diff\n", encoding="utf-8")
        review_artifacts = [
            {
                "label": "runtime",
                "path": str(runtime_review_artifact),
                "sha256": file_sha256(runtime_review_artifact),
                "size_bytes": runtime_review_artifact.stat().st_size,
            },
            {
                "label": "codex_desktop",
                "path": str(desktop_review_artifact),
                "sha256": file_sha256(desktop_review_artifact),
                "size_bytes": desktop_review_artifact.stat().st_size,
            },
        ]
        write_json(
            marker,
            {
                "schema": "agent-workspace-linux.human_final_diff_review.v1",
                "source_identity": source_identity,
                "status": "reviewed",
                "reviewed_at_utc": stale_stamp,
                "reviewer": "release-gate-self-test",
                "notes": "Self-test human review accepted the generated runtime and Desktop diff artifacts.",
                "review_artifacts": review_artifacts,
            },
        )

        stale = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        assert_self_test(stale["status"] == "pending", "stale evidence must stay pending")

        mismatched_identity = dict(source_identity)
        mismatched_identity["source_hash"] = "wrong-source"
        gnome_report = json.loads((viewer_dir / "gnome-x11.json").read_text(encoding="utf-8"))
        gnome_wayland_report = json.loads(
            (viewer_dir / "gnome-wayland-xwayland-negative.json").read_text(encoding="utf-8")
        )
        gnome_report["source_identity"] = mismatched_identity
        gnome_wayland_report["source_identity"] = mismatched_identity
        write_json(viewer_dir / "gnome-x11.json", gnome_report)
        write_json(viewer_dir / "gnome-wayland-xwayland-negative.json", gnome_wayland_report)
        mismatch = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=0,
            source_identity=source_identity,
            require_clean_source=False,
        )
        assert_self_test(
            mismatch["status"] == "pending", "source identity mismatch must stay pending"
        )
        gnome_report["source_identity"] = source_identity
        gnome_wayland_report["source_identity"] = source_identity
        write_json(viewer_dir / "gnome-x11.json", gnome_report)
        write_json(viewer_dir / "gnome-wayland-xwayland-negative.json", gnome_wayland_report)

        for path in [
            viewer_dir / "gnome-x11.json",
            viewer_dir / "kde-wayland.json",
            viewer_dir / "gnome-wayland-xwayland-negative.json",
        ]:
            report = json.loads(path.read_text(encoding="utf-8"))
            report["created_at_utc"] = fresh_stamp
            report["source_identity"] = source_identity
            write_json(path, report)
        grocery_report = json.loads(
            (grocery_dir / "real-browser.json").read_text(encoding="utf-8")
        )
        grocery_report["created_at_utc"] = fresh_stamp
        grocery_report["source_identity"] = source_identity
        write_json(grocery_dir / "real-browser.json", grocery_report)
        app_qa_report = json.loads((app_qa_dir / "local-gui.json").read_text(encoding="utf-8"))
        app_qa_report["created_at_utc"] = fresh_stamp
        app_qa_report["source_identity"] = source_identity
        write_json(app_qa_dir / "local-gui.json", app_qa_report)
        marker_report = json.loads(marker.read_text(encoding="utf-8"))
        marker_report["reviewed_at_utc"] = fresh_stamp
        marker_report["source_identity"] = source_identity
        write_json(marker, marker_report)

        negative_native_report = json.loads(
            (viewer_dir / "gnome-wayland-xwayland-negative.json").read_text(encoding="utf-8")
        )
        assert_self_test(
            not native_wayland_observed(negative_native_report),
            "GNOME/Xwayland negative observation must not count as native Wayland layer-shell evidence",
        )
        assert_self_test(
            x11_xwayland_viewer_protocol_observed(negative_native_report),
            "GNOME Wayland Xwayland smoke should count as X11/Xwayland viewer protocol evidence",
        )

        grocery_report = json.loads(
            (grocery_dir / "real-browser.json").read_text(encoding="utf-8")
        )
        valid_chrome_devtools = dict(grocery_report["real_browser"]["chrome_devtools"])
        grocery_report["real_browser"].pop("chrome_devtools")
        write_json(grocery_dir / "real-browser.json", grocery_report)
        missing_workspace_browser = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        grocery_gate = next(
            gate
            for gate in missing_workspace_browser["gates"]
            if gate["id"] == "real_grocery_dogfood"
        )
        assert_self_test(
            "real-browser grocery report must prove workspace-owned browser target discovery and page snapshot through loopback DevTools"
            in grocery_gate["missing"],
            "real grocery release evidence must require workspace-owned browser target discovery and page snapshot",
        )
        grocery_report["real_browser"]["chrome_devtools"] = valid_chrome_devtools
        write_json(grocery_dir / "real-browser.json", grocery_report)

        privacy_leak_grocery = json.loads(json.dumps(grocery_report))
        privacy_leak_grocery["real_browser"]["chrome_devtools"]["page_snapshot"][
            "text_excerpt"
        ] = "Private address 123 Main Street"
        write_json(grocery_dir / "real-browser.json", privacy_leak_grocery)
        grocery_privacy = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        grocery_gate = next(
            gate for gate in grocery_privacy["gates"] if gate["id"] == "real_grocery_dogfood"
        )
        assert_self_test(
            "real-browser grocery release evidence must omit raw logged-in page text"
            in grocery_gate["missing"],
            "real grocery release evidence must reject raw logged-in page text",
        )
        write_json(grocery_dir / "real-browser.json", grocery_report)

        kde_report = json.loads((viewer_dir / "kde-wayland.json").read_text(encoding="utf-8"))
        original_kde_session = dict(kde_report["session"])
        kde_report["session"]["xdg_session_type"] = "x11"
        write_json(viewer_dir / "kde-wayland.json", kde_report)
        x11_native_spoof = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        viewer_gate = next(
            gate for gate in x11_native_spoof["gates"] if gate["id"] == "viewer_desktop_matrix"
        )
        assert_self_test(
            "native Wayland layer-shell/compositor observation with notes"
            in viewer_gate.get("advisory_missing", []),
            "native Wayland spoof must be tracked only as advisory follow-up",
        )
        kde_report["session"] = original_kde_session
        write_json(viewer_dir / "kde-wayland.json", kde_report)

        grocery_report = json.loads(
            (grocery_dir / "real-browser.json").read_text(encoding="utf-8")
        )
        grocery_report["inputs"]["target_url"] = "https://localhost/groceries"
        write_json(grocery_dir / "real-browser.json", grocery_report)
        local_grocery = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        grocery_gate = next(
            gate for gate in local_grocery["gates"] if gate["id"] == "real_grocery_dogfood"
        )
        assert_self_test(
            "real-browser grocery target URL must be an HTTPS non-local grocery site"
            in grocery_gate["missing"],
            "real grocery release evidence must not accept localhost targets",
        )
        grocery_report["inputs"][
            "target_url"
        ] = "https://grocery-release-gate.example-retailer.com"
        write_json(grocery_dir / "real-browser.json", grocery_report)

        grocery_report["real_browser"]["profile_directory"] = "Default"
        write_json(grocery_dir / "real-browser.json", grocery_report)
        mismatched_profile_grocery = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        grocery_gate = next(
            gate
            for gate in mismatched_profile_grocery["gates"]
            if gate["id"] == "real_grocery_dogfood"
        )
        assert_self_test(
            "real-browser grocery report must have consistent safe Chrome profile directory evidence when a profile directory is requested"
            in grocery_gate["missing"],
            "real grocery release evidence must reject mismatched Chrome profile directories",
        )
        grocery_report["real_browser"]["profile_directory"] = "Profile 1"
        grocery_report["inputs"]["profile_directory"] = "../Profile 1"
        write_json(grocery_dir / "real-browser.json", grocery_report)
        unsafe_profile_grocery = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        grocery_gate = next(
            gate
            for gate in unsafe_profile_grocery["gates"]
            if gate["id"] == "real_grocery_dogfood"
        )
        assert_self_test(
            "real-browser grocery report must have consistent safe Chrome profile directory evidence when a profile directory is requested"
            in grocery_gate["missing"],
            "real grocery release evidence must reject unsafe Chrome profile directory strings",
        )
        grocery_report["inputs"]["profile_directory"] = "Profile 1"
        write_json(grocery_dir / "real-browser.json", grocery_report)

        grocery_report["safety_contract"]["real_browser_allows_only_declared_cart_draft_input"] = False
        write_json(grocery_dir / "real-browser.json", grocery_report)
        unsafe_grocery = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        grocery_gate = next(
            gate for gate in unsafe_grocery["gates"] if gate["id"] == "real_grocery_dogfood"
        )
        assert_self_test(
            "real-browser grocery report must include passed plan assertions and cart-draft safety contract"
            in grocery_gate["missing"],
            "real grocery release evidence must require the cart-draft safety contract",
        )
        grocery_report["safety_contract"]["real_browser_allows_only_declared_cart_draft_input"] = True
        grocery_report["plan_assertions"]["checkout_still_blocked_after_cart_approval"] = False
        write_json(grocery_dir / "real-browser.json", grocery_report)
        weak_plan_grocery = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        grocery_gate = next(
            gate for gate in weak_plan_grocery["gates"] if gate["id"] == "real_grocery_dogfood"
        )
        assert_self_test(
            "real-browser grocery report must include passed plan assertions and cart-draft safety contract"
            in grocery_gate["missing"],
            "real grocery release evidence must require checkout-blocked plan assertions",
        )
        grocery_report["plan_assertions"]["checkout_still_blocked_after_cart_approval"] = True
        write_json(grocery_dir / "real-browser.json", grocery_report)

        grocery_report["real_browser"]["workspace_input_audit"]["input_event_count"] = 4
        grocery_report["real_browser"]["workspace_input_audit"]["input_event_kinds"] = [
            "key_window",
            "kill_app",
            "paste_window",
        ]
        grocery_report["real_browser"]["workspace_input_audit"]["input_event_sequences"] = [
            10,
            11,
            12,
            13,
        ]
        grocery_report["real_browser"]["workspace_input_audit"]["unexpected_input_event_count"] = 1
        grocery_report["real_browser"]["workspace_input_audit"]["unexpected_input_event_kinds"] = [
            "kill_app"
        ]
        grocery_report["real_browser"]["workspace_input_audit"]["unexpected_input_event_sequences"] = [
            13
        ]
        write_json(grocery_dir / "real-browser.json", grocery_report)
        input_grocery = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        grocery_gate = next(
            gate for gate in input_grocery["gates"] if gate["id"] == "real_grocery_dogfood"
        )
        assert_self_test(
            "real-browser grocery report must prove only declared cart-draft workspace input events"
            in grocery_gate["missing"],
            "real grocery release evidence must reject unexpected workspace input events",
        )
        grocery_report["real_browser"]["workspace_input_audit"] = {
            "checked": True,
            "event_scope": "since_sequence",
            "events_since_sequence": 9,
            "events_tail_requested": 120,
            "minimum_events_tail_required": 120,
            "total_events": 3,
            "expected_input_step_count": 3,
            "input_event_count": 3,
            "input_event_count_covers_expected": True,
            "input_event_kinds": ["key_window", "paste_window"],
            "input_event_sequences": [10, 11, 12],
            "allowed_input_event_kinds": [
                "click_window",
                "key_window",
                "paste_window",
                "scroll_window",
                "type_window",
            ],
            "unexpected_input_event_count": 0,
            "unexpected_input_event_kinds": [],
            "unexpected_input_event_sequences": [],
        }
        write_json(grocery_dir / "real-browser.json", grocery_report)

        grocery_report["real_browser"]["workspace_input_audit"]["input_event_count"] = 2
        grocery_report["real_browser"]["workspace_input_audit"][
            "input_event_count_covers_expected"
        ] = False
        grocery_report["real_browser"]["workspace_input_audit"][
            "input_event_sequences"
        ] = [10, 11]
        write_json(grocery_dir / "real-browser.json", grocery_report)
        incomplete_input_grocery = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        grocery_gate = next(
            gate for gate in incomplete_input_grocery["gates"] if gate["id"] == "real_grocery_dogfood"
        )
        assert_self_test(
            "real-browser grocery report must prove only declared cart-draft workspace input events"
            in grocery_gate["missing"],
            "real grocery release evidence must reject missing declared input events",
        )
        grocery_report["real_browser"]["workspace_input_audit"]["input_event_count"] = 3
        grocery_report["real_browser"]["workspace_input_audit"][
            "input_event_count_covers_expected"
        ] = True
        grocery_report["real_browser"]["workspace_input_audit"][
            "input_event_sequences"
        ] = [10, 11, 12]

        grocery_report["real_browser"]["workspace_input_audit"][
            "events_tail_requested"
        ] = 30
        write_json(grocery_dir / "real-browser.json", grocery_report)
        shallow_tail_grocery = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        grocery_gate = next(
            gate for gate in shallow_tail_grocery["gates"] if gate["id"] == "real_grocery_dogfood"
        )
        assert_self_test(
            "real-browser grocery report must prove only declared cart-draft workspace input events"
            in grocery_gate["missing"],
            "real grocery release evidence must reject too-shallow event tails",
        )
        grocery_report["real_browser"]["workspace_input_audit"][
            "events_tail_requested"
        ] = 120
        write_json(grocery_dir / "real-browser.json", grocery_report)

        valid_cleanup = dict(grocery_report["real_browser"]["cleanup"])
        grocery_report["real_browser"]["cleanup"] = {
            "dry_run": False,
            "removed": [],
            "skipped": [],
        }
        write_json(grocery_dir / "real-browser.json", grocery_report)
        missing_cleanup_grocery = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        grocery_gate = next(
            gate
            for gate in missing_cleanup_grocery["gates"]
            if gate["id"] == "real_grocery_dogfood"
        )
        assert_self_test(
            "real-browser grocery report must prove the stopped workspace runtime was cleaned up"
            in grocery_gate["missing"],
            "real grocery release evidence must reject reports that do not clean the workspace runtime",
        )
        grocery_report["real_browser"]["cleanup"] = valid_cleanup
        grocery_report["real_browser"]["workspace_preserved_for_debug"] = True
        write_json(grocery_dir / "real-browser.json", grocery_report)
        preserved_cleanup_grocery = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        grocery_gate = next(
            gate
            for gate in preserved_cleanup_grocery["gates"]
            if gate["id"] == "real_grocery_dogfood"
        )
        assert_self_test(
            "real-browser grocery report must prove the stopped workspace runtime was cleaned up"
            in grocery_gate["missing"],
            "real grocery release evidence must reject debug-preserved workspace runtimes",
        )
        grocery_report["real_browser"].pop("workspace_preserved_for_debug")
        grocery_report["real_browser"]["cleanup"] = valid_cleanup
        write_json(grocery_dir / "real-browser.json", grocery_report)

        valid_executed_steps = list(
            grocery_report["real_browser"]["cart_draft_interaction"]["executed_steps"]
        )
        grocery_report["real_browser"]["cart_draft_interaction"]["executed_steps"] = []
        write_json(grocery_dir / "real-browser.json", grocery_report)
        missing_steps_grocery = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        grocery_gate = next(
            gate for gate in missing_steps_grocery["gates"] if gate["id"] == "real_grocery_dogfood"
        )
        assert_self_test(
            "real-browser grocery report must include a passed cart-draft interaction with at least one cart mutation step"
            in grocery_gate["missing"],
            "real grocery release evidence must include executed cart-draft step evidence",
        )
        grocery_report["real_browser"]["cart_draft_interaction"]["executed_steps"] = [
            dict(step) for step in valid_executed_steps
        ]
        grocery_report["real_browser"]["cart_draft_interaction"]["executed_steps"][2][
            "safety_label"
        ] = "Click checkout to place order"
        write_json(grocery_dir / "real-browser.json", grocery_report)
        forbidden_steps_grocery = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        grocery_gate = next(
            gate for gate in forbidden_steps_grocery["gates"] if gate["id"] == "real_grocery_dogfood"
        )
        assert_self_test(
            "real-browser grocery report must include a passed cart-draft interaction with at least one cart mutation step"
            in grocery_gate["missing"],
            "real grocery release evidence must reject executed checkout/payment/account step labels",
        )
        grocery_report["real_browser"]["cart_draft_interaction"]["executed_steps"] = valid_executed_steps
        write_json(grocery_dir / "real-browser.json", grocery_report)

        grocery_report["real_browser"]["cart_draft_interaction"]["executed_steps"] = [
            dict(step) for step in valid_executed_steps
        ]
        grocery_report["real_browser"]["cart_draft_interaction"]["executed_steps"][1][
            "result"
        ] = {"ok": False, "message": "input failed"}
        write_json(grocery_dir / "real-browser.json", grocery_report)
        failed_step_grocery = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        grocery_gate = next(
            gate for gate in failed_step_grocery["gates"] if gate["id"] == "real_grocery_dogfood"
        )
        assert_self_test(
            "real-browser grocery report must include a passed cart-draft interaction with at least one cart mutation step"
            in grocery_gate["missing"],
            "real grocery release evidence must reject failed executed cart-draft steps",
        )
        grocery_report["real_browser"]["cart_draft_interaction"]["executed_steps"] = valid_executed_steps
        write_json(grocery_dir / "real-browser.json", grocery_report)

        kde_report = json.loads((viewer_dir / "kde-wayland.json").read_text(encoding="utf-8"))
        gnome_report = json.loads((viewer_dir / "gnome-x11.json").read_text(encoding="utf-8"))
        gnome_wayland_report = json.loads(
            (viewer_dir / "gnome-wayland-xwayland-negative.json").read_text(encoding="utf-8")
        )
        original_gnome_matrix = json.loads(json.dumps(gnome_report["matrix_result"]))
        original_gnome_wayland_matrix = json.loads(
            json.dumps(gnome_wayland_report["matrix_result"])
        )
        nested_display_attestation = {
            "release_eligible": False,
            "problems": ["display server appears nested or headless"],
            "warnings": [],
            "display_protocols": ["x11"],
            "sockets": [
                {
                    "kind": "x11",
                    "path": "/tmp/.X11-unix/X99",
                    "exists": True,
                    "processes": [{"command": "Xvfb", "pid": 999, "args": "Xvfb :99"}],
                }
            ],
            "known_nested_or_headless_processes": [
                {"command": "Xvfb", "pid": 999, "args": "Xvfb :99"}
            ],
            "lsof_available": True,
        }
        gnome_report["matrix_result"]["display_attestation"] = nested_display_attestation
        gnome_wayland_report["matrix_result"]["display_attestation"] = nested_display_attestation
        write_json(viewer_dir / "gnome-x11.json", gnome_report)
        write_json(viewer_dir / "gnome-wayland-xwayland-negative.json", gnome_wayland_report)
        nested_display = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        viewer_gate = next(
            gate for gate in nested_display["gates"] if gate["id"] == "viewer_desktop_matrix"
        )
        assert_self_test(
            "Linux desktop viewer smoke row" in viewer_gate["missing"]
            or "X11/Xwayland viewer protocol evidence" in viewer_gate["missing"],
            "viewer evidence from nested/headless display servers must not count",
        )
        gnome_report["matrix_result"] = json.loads(json.dumps(original_gnome_matrix))
        gnome_wayland_report["matrix_result"] = json.loads(
            json.dumps(original_gnome_wayland_matrix)
        )
        write_json(viewer_dir / "gnome-x11.json", gnome_report)
        write_json(viewer_dir / "gnome-wayland-xwayland-negative.json", gnome_wayland_report)

        weak_display_attestation = {
            "release_eligible": True,
            "problems": [],
            "warnings": [],
            "display_protocols": ["x11"],
            "sockets": [],
            "known_nested_or_headless_processes": [],
            "lsof_available": True,
        }
        gnome_report["matrix_result"]["display_attestation"] = weak_display_attestation
        gnome_wayland_report["matrix_result"]["display_attestation"] = weak_display_attestation
        write_json(viewer_dir / "gnome-x11.json", gnome_report)
        write_json(viewer_dir / "gnome-wayland-xwayland-negative.json", gnome_wayland_report)
        weak_display = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        viewer_gate = next(
            gate for gate in weak_display["gates"] if gate["id"] == "viewer_desktop_matrix"
        )
        assert_self_test(
            "Linux desktop viewer smoke row" in viewer_gate["missing"]
            or "X11/Xwayland viewer protocol evidence" in viewer_gate["missing"],
            "viewer evidence without display socket/process proof must not count",
        )
        gnome_report["matrix_result"] = json.loads(json.dumps(original_gnome_matrix))
        gnome_wayland_report["matrix_result"] = json.loads(
            json.dumps(original_gnome_wayland_matrix)
        )
        write_json(viewer_dir / "gnome-x11.json", gnome_report)
        write_json(viewer_dir / "gnome-wayland-xwayland-negative.json", gnome_wayland_report)

        original_kde_matrix = json.loads(json.dumps(kde_report["matrix_result"]))
        kde_report["matrix_result"]["session_consistency"] = {
            "release_eligible": False,
            "problems": ["XDG_SESSION_TYPE='x11' conflicts with loginctl Type='wayland'"],
        }
        write_json(viewer_dir / "kde-wayland.json", kde_report)
        spoofed_session = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        viewer_gate = next(
            gate for gate in spoofed_session["gates"] if gate["id"] == "viewer_desktop_matrix"
        )
        assert_self_test(
            "KDE/Plasma viewer smoke row" in viewer_gate.get("advisory_missing", [])
            and "native Wayland layer-shell/compositor observation with notes"
            in viewer_gate.get("advisory_missing", []),
            "viewer evidence with contradictory session attestation should leave external rows as advisory follow-up",
        )
        kde_report["matrix_result"] = json.loads(json.dumps(original_kde_matrix))
        write_json(viewer_dir / "kde-wayland.json", kde_report)

        app_qa_report = json.loads((app_qa_dir / "local-gui.json").read_text(encoding="utf-8"))
        app_qa_report["safety_contract"]["host_desktop_input_targeted"] = True
        write_json(app_qa_dir / "local-gui.json", app_qa_report)
        unsafe_app_qa = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        app_qa_gate = next(
            gate for gate in unsafe_app_qa["gates"] if gate["id"] == "app_qa_dogfood"
        )
        assert_self_test(
            any("non-destructive input" in missing for missing in app_qa_gate["missing"]),
            "app-QA dogfood evidence must reject host desktop or real-world mutation drift",
        )
        app_qa_report["safety_contract"]["host_desktop_input_targeted"] = False
        write_json(app_qa_dir / "local-gui.json", app_qa_report)

        passed = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
        )
        assert_self_test(passed["status"] == "passed", "complete evidence should pass")
        assert_self_test(
            all(gate["status"] == "passed" for gate in passed["gates"]),
            "each gate should pass with complete evidence",
        )

        marker_report = json.loads(marker.read_text(encoding="utf-8"))
        marker_report["review_scope_identity"] = {
            **review_scope_identity,
            "review_scope_hash": "wrong-review-scope",
        }
        write_json(marker, marker_report)
        review_scope_mismatch = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
            review_scope_identity=review_scope_identity,
        )
        human_gate = next(
            gate for gate in review_scope_mismatch["gates"] if gate["id"] == "human_final_diff_review"
        )
        assert_self_test(
            any("review scope" in missing for missing in human_gate["missing"]),
            "human review marker must match the current review scope",
        )
        marker_report["review_scope_identity"] = review_scope_identity
        write_json(marker, marker_report)
        review_scope_passed = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
            review_scope_identity=review_scope_identity,
        )
        assert_self_test(
            review_scope_passed["status"] == "passed",
            "matching review scope should allow complete evidence to pass",
        )

        marker_report = json.loads(marker.read_text(encoding="utf-8"))
        marker_report["notes"] = "<what was reviewed and accepted>"
        write_json(marker, marker_report)
        placeholder_notes = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
            review_scope_identity=review_scope_identity,
        )
        human_gate = next(
            gate for gate in placeholder_notes["gates"] if gate["id"] == "human_final_diff_review"
        )
        assert_self_test(
            any("reviewer and notes" in missing for missing in human_gate["missing"]),
            "human review marker must reject placeholder review notes",
        )
        marker_report["notes"] = (
            "Self-test human review accepted the generated runtime and Desktop diff artifacts."
        )
        write_json(marker, marker_report)

        marker_report = json.loads(marker.read_text(encoding="utf-8"))
        marker_report["review_artifacts"][0]["sha256"] = "0" * 64
        write_json(marker, marker_report)
        review_artifact_mismatch = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            require_clean_source=False,
            review_scope_identity=review_scope_identity,
        )
        human_gate = next(
            gate
            for gate in review_artifact_mismatch["gates"]
            if gate["id"] == "human_final_diff_review"
        )
        assert_self_test(
            any("review_artifacts" in missing for missing in human_gate["missing"]),
            "human review marker must bind to generated review artifact hashes",
        )
        marker_report["review_artifacts"] = review_artifacts
        write_json(marker, marker_report)

        dirty_source = dict(source_identity)
        dirty_source["source_dirty_count"] = 2
        dirty = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=dirty_source,
            require_clean_source=True,
        )
        assert_self_test(dirty["status"] == "pending", "dirty source must block clean release")

        clean_source = dict(source_identity)
        clean_source["source_dirty_count"] = 0
        for path in [
            viewer_dir / "gnome-x11.json",
            viewer_dir / "kde-wayland.json",
            viewer_dir / "gnome-wayland-xwayland-negative.json",
        ]:
            report = json.loads(path.read_text(encoding="utf-8"))
            report["source_identity"] = clean_source
            write_json(path, report)
        grocery_report = json.loads(
            (grocery_dir / "real-browser.json").read_text(encoding="utf-8")
        )
        grocery_report["source_identity"] = clean_source
        write_json(grocery_dir / "real-browser.json", grocery_report)
        app_qa_report = json.loads((app_qa_dir / "local-gui.json").read_text(encoding="utf-8"))
        app_qa_report["source_identity"] = clean_source
        write_json(app_qa_dir / "local-gui.json", app_qa_report)
        marker_report = json.loads(marker.read_text(encoding="utf-8"))
        marker_report["source_identity"] = clean_source
        write_json(marker, marker_report)
        clean = build_report(
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            human_review_marker=marker,
            now=now,
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=clean_source,
            require_clean_source=True,
        )
        assert_self_test(clean["status"] == "passed", "clean source should pass strict release")

    print("release gate audit self-test passed")


def print_summary(report: dict[str, Any], report_path: Path) -> None:
    print(f"release gate audit report: {report_path}")
    print(f"release gate audit status: {report['status']}")
    for gate in report["gates"]:
        if gate["status"] == "passed":
            print(f"- {gate['id']}: passed")
            continue
        print(f"- {gate['id']}: pending")
        for missing in gate["missing"]:
            print(f"  missing: {missing}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=DEFAULT_OUTPUT_DIR,
        help="directory for the audit JSON report",
    )
    parser.add_argument(
        "--viewer-dir",
        type=Path,
        default=DEFAULT_VIEWER_DIR,
        help="directory containing viewer desktop matrix JSON reports",
    )
    parser.add_argument(
        "--grocery-dir",
        type=Path,
        default=DEFAULT_GROCERY_DIR,
        help="legacy directory containing optional real grocery dogfood JSON reports",
    )
    parser.add_argument(
        "--github-explore-dir",
        type=Path,
        default=DEFAULT_GITHUB_EXPLORE_DIR,
        help="directory containing GitHub Explore dogfood JSON reports",
    )
    parser.add_argument(
        "--include-legacy-grocery",
        action="store_true",
        help="include the legacy real-grocery dogfood gate in addition to the GitHub Explore gate",
    )
    parser.add_argument(
        "--app-qa-dir",
        type=Path,
        default=DEFAULT_APP_QA_DIR,
        help="directory containing local app-QA dogfood JSON reports",
    )
    parser.add_argument(
        "--require-all",
        action="store_true",
        help="exit non-zero unless all release-only gates are proven",
    )
    parser.add_argument(
        "--human-review-marker",
        type=Path,
        default=DEFAULT_HUMAN_REVIEW_MARKER,
        help="JSON marker proving final human diff review",
    )
    parser.add_argument(
        "--desktop-repo",
        type=Path,
        default=DEFAULT_DESKTOP_REPO,
        help="sibling Codex Desktop repo whose agent-workspace feature source is part of release identity",
    )
    parser.add_argument(
        "--max-evidence-age-days",
        type=int,
        default=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
        help="maximum age for release evidence; use 0 to disable freshness checks",
    )
    parser.add_argument(
        "--no-source-identity-check",
        action="store_true",
        help="do not require evidence or human review markers to match the current combined source/review identity",
    )
    parser.add_argument(
        "--require-clean-source",
        action="store_true",
        help="require runtime source paths and sibling Desktop feature paths to have no git status entries",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run synthetic fixture checks for pending and passing release states",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.self_test:
        run_self_test()
        return 0
    args.output_dir.mkdir(parents=True, exist_ok=True)
    now = dt.datetime.now(dt.timezone.utc)
    stamp = now.strftime("%Y%m%dT%H%M%SZ")
    report_path = args.output_dir / f"{stamp}.json"
    source_identity = (
        None
        if args.no_source_identity_check
        else compute_source_identity(ROOT, desktop_repo=args.desktop_repo)
    )
    review_scope_identity = (
        None
        if args.no_source_identity_check
        else compute_review_scope_identity(ROOT, desktop_repo=args.desktop_repo)
    )
    report = build_report(
        viewer_dir=args.viewer_dir,
        app_qa_dir=args.app_qa_dir,
        grocery_dir=args.grocery_dir,
        github_explore_dir=args.github_explore_dir,
        human_review_marker=args.human_review_marker,
        now=now,
        max_evidence_age_days=args.max_evidence_age_days,
        source_identity=source_identity,
        require_clean_source=args.require_clean_source,
        review_scope_identity=review_scope_identity,
        include_legacy_grocery=args.include_legacy_grocery,
    )
    write_json(report_path, report)
    print_summary(report, report_path)
    if args.require_all and report["status"] != "passed":
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
