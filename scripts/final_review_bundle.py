#!/usr/bin/env python3
"""Create a review bundle for the final human release gate."""

from __future__ import annotations

import datetime as dt
import hashlib
import json
import os
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path
from typing import Any

sys.dont_write_bytecode = True

from release_gate_audit import DEFAULT_DESKTOP_REPO
from release_gate_audit import DEFAULT_HUMAN_REVIEW_MARKER
from release_gate_audit import DESKTOP_SOURCE_IDENTITY_PATHS
from release_gate_audit import RUNTIME_SOURCE_IDENTITY_PATHS
from release_gate_audit import compute_desktop_source_identity
from release_gate_audit import compute_repo_source_identity
from release_gate_audit import compute_review_scope_identity
from release_gate_audit import compute_runtime_source_identity
from release_gate_audit import compute_source_identity
from release_gate_audit import source_file_paths


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_OUTPUT_DIR = ROOT / "target" / "final-review-bundle"
DEFAULT_SOURCE_BUNDLE_DIR = ROOT / "target" / "release-evidence-source-bundle"
SOURCE_BUNDLE_MANIFEST = "release-evidence-source-bundle.json"


def run_command(command: list[str], cwd: Path, timeout: int = 10) -> dict[str, Any]:
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            check=False,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except Exception as error:
        return {"ok": False, "error": str(error), "command": command}
    return {
        "ok": completed.returncode == 0,
        "exit_code": completed.returncode,
        "stdout": completed.stdout.strip(),
        "stderr": completed.stderr.strip(),
        "command": command,
    }


def run_raw_command(command: list[str], cwd: Path, timeout: int = 30) -> dict[str, Any]:
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            check=False,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except Exception as error:
        return {
            "command": command,
            "exit_code": None,
            "stdout": "",
            "stderr": str(error),
        }
    return {
        "command": command,
        "exit_code": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }


def git(cwd: Path, args: list[str], timeout: int = 10) -> str | None:
    result = run_command(["git", *args], cwd, timeout=timeout)
    return result["stdout"] if result.get("ok") else None


def split_lines(value: str | None) -> list[str]:
    return [line for line in (value or "").splitlines() if line.strip()]


def latest_file(directory: Path, pattern: str) -> Path | None:
    if not directory.exists():
        return None
    files = sorted(directory.glob(pattern))
    return files[-1] if files else None


def latest_json(directory: Path) -> Path | None:
    return latest_file(directory, "*.json")


def read_json(path: Path | None) -> dict[str, Any] | None:
    if path is None:
        return None
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None
    return value if isinstance(value, dict) else None


def read_source_bundle_manifest(path: Path | None) -> dict[str, Any] | None:
    if path is None:
        return None
    try:
        with tarfile.open(path, "r:gz") as archive:
            member = next(
                (
                    item
                    for item in archive.getmembers()
                    if item.isfile() and item.name.endswith(f"/{SOURCE_BUNDLE_MANIFEST}")
                ),
                None,
            )
            if member is None:
                return None
            fileobj = archive.extractfile(member)
            if fileobj is None:
                return None
            value = json.loads(fileobj.read().decode("utf-8"))
    except Exception:
        return None
    return value if isinstance(value, dict) else None


def repo_snapshot(
    path: Path,
    *,
    source_paths: list[str] | None = None,
    source_label: str | None = None,
) -> dict[str, Any]:
    exists = path.exists()
    snapshot: dict[str, Any] = {
        "path": str(path),
        "exists": exists,
    }
    if not exists:
        if source_paths is not None and source_label is not None:
            snapshot["source_identity"] = compute_repo_source_identity(
                path,
                source_paths,
                label=source_label,
            )
        return snapshot
    status_short = split_lines(git(path, ["status", "--short"]))
    snapshot.update(
        {
            "branch": git(path, ["branch", "--show-current"]),
            "git_head": git(path, ["rev-parse", "HEAD"]),
            "status_short": status_short,
            "dirty_count": len(status_short),
            "diff_stat": git(path, ["diff", "--stat"]),
            "diff_name_status": split_lines(git(path, ["diff", "--name-status"])),
            "untracked": split_lines(git(path, ["ls-files", "--others", "--exclude-standard"])),
        }
    )
    if source_paths is not None and source_label is not None:
        snapshot["source_identity"] = compute_repo_source_identity(
            path,
            source_paths,
            label=source_label,
        )
    return snapshot


def latest_evidence() -> dict[str, Any]:
    evidence_dirs = {
        "app_qa_dogfood": ROOT / "target" / "app-qa-dogfood",
        "github_explore_dogfood": ROOT / "target" / "github-explore-dogfood",
        "viewer_desktop_matrix": ROOT / "target" / "viewer-desktop-matrix",
        "release_gate_audit": ROOT / "target" / "release-gate-audit",
    }
    evidence = {}
    for key, directory in evidence_dirs.items():
        path = latest_json(directory)
        evidence[key] = {
            "path": str(path) if path else None,
            "report": read_json(path),
        }
    source_bundle = latest_file(DEFAULT_SOURCE_BUNDLE_DIR, "*.tar.gz")
    evidence["release_evidence_source_bundle"] = {
        "path": str(source_bundle) if source_bundle else None,
        "manifest": read_source_bundle_manifest(source_bundle),
    }
    return evidence


def marker_template(
    source_identity: dict[str, Any],
    review_scope_identity: dict[str, Any],
    review_artifacts: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    return {
        "schema": "agent-workspace-linux.human_final_diff_review.v1",
        "status": "reviewed",
        "reviewed_at_utc": "<fill with current UTC ISO timestamp>",
        "reviewer": "<human reviewer>",
        "notes": "<scope, concerns, or approval notes>",
        "source_identity": source_identity,
        "review_scope_identity": review_scope_identity,
        "review_artifacts": (
            review_artifacts
            if review_artifacts is not None
            else "<generated runtime/Desktop review artifact list>"
        ),
    }


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def append_command_section(lines: list[str], title: str, result: dict[str, Any]) -> None:
    lines.extend(
        [
            "",
            f"## {title}",
            "",
            "```text",
            f"$ {' '.join(str(part) for part in result.get('command') or [])}",
            f"exit_code: {result.get('exit_code')}",
        ]
    )
    stdout = result.get("stdout") or ""
    stderr = result.get("stderr") or ""
    if stdout:
        lines.extend(["", stdout.rstrip()])
    if stderr:
        lines.extend(["", "--- stderr ---", stderr.rstrip()])
    lines.append("```")


def is_git_worktree(repo_path: Path) -> bool:
    return git(repo_path, ["rev-parse", "--is-inside-work-tree"]) == "true"


def append_source_inventory_section(
    lines: list[str],
    repo_path: Path,
    source_paths: list[str],
) -> None:
    paths = source_file_paths(repo_path, source_paths)
    lines.extend(
        [
            "",
            "## No Git Repository Source Inventory",
            "",
            "This source snapshot has no usable `.git` metadata, so this artifact",
            "records the manifest-scoped source files directly for review.",
            "",
            "```text",
        ]
    )
    if not paths:
        lines.append("<no source files found>")
    for rel_path in paths:
        full_path = repo_path / rel_path
        if not full_path.is_file():
            continue
        lines.append(f"{rel_path}\t{full_path.stat().st_size}\t{file_sha256(full_path)}")
    lines.append("```")

    for rel_path in paths:
        full_path = repo_path / rel_path
        if not full_path.is_file():
            continue
        append_command_section(
            lines,
            f"Source File Patch: {rel_path}",
            run_raw_command(
                ["git", "diff", "--no-index", "--", "/dev/null", rel_path],
                repo_path,
                timeout=30,
            ),
        )


def write_repo_review_diff(
    repo_path: Path,
    output_path: Path,
    *,
    label: str,
    source_paths: list[str] | None = None,
) -> dict[str, Any]:
    lines = [
        f"# Final Human Review Diff: {label}",
        "",
        f"Generated: {dt.datetime.now(dt.timezone.utc).isoformat()}",
        f"Repo: {repo_path}",
        "",
        "This artifact is for human review only. It records repository status,",
        "staged and unstaged diffs, and patch-form views of non-ignored",
        "untracked files so the review marker can be based on concrete bytes.",
    ]
    if not repo_path.exists():
        lines.extend(["", "Repository path does not exist."])
    elif not is_git_worktree(repo_path):
        append_source_inventory_section(lines, repo_path, source_paths or [])
    else:
        sections = [
            ("Status", ["git", "status", "--short"]),
            ("Staged Diff Stat", ["git", "diff", "--cached", "--stat"]),
            (
                "Staged Diff",
                ["git", "diff", "--cached", "--binary", "--no-ext-diff"],
            ),
            ("Unstaged Diff Stat", ["git", "diff", "--stat"]),
            ("Unstaged Diff", ["git", "diff", "--binary", "--no-ext-diff"]),
        ]
        for title, command in sections:
            append_command_section(lines, title, run_raw_command(command, repo_path))

        untracked = split_lines(
            git(repo_path, ["ls-files", "--others", "--exclude-standard"]) or ""
        )
        lines.extend(["", "## Untracked Files", "", "```text"])
        lines.extend(untracked or ["<none>"])
        lines.append("```")
        for rel_path in untracked:
            append_command_section(
                lines,
                f"Untracked File Patch: {rel_path}",
                run_raw_command(
                    ["git", "diff", "--no-index", "--", "/dev/null", rel_path],
                    repo_path,
                ),
            )
    output_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return {
        "label": label,
        "path": str(output_path),
        "sha256": file_sha256(output_path),
        "size_bytes": output_path.stat().st_size,
    }


def write_review_artifacts(
    output_dir: Path,
    stamp: str,
    *,
    desktop_repo: Path,
) -> list[dict[str, Any]]:
    return [
        write_repo_review_diff(
            ROOT,
            output_dir / f"{stamp}-runtime-review.diff",
            label="runtime",
            source_paths=RUNTIME_SOURCE_IDENTITY_PATHS,
        ),
        write_repo_review_diff(
            desktop_repo,
            output_dir / f"{stamp}-codex-desktop-review.diff",
            label="codex_desktop",
            source_paths=DESKTOP_SOURCE_IDENTITY_PATHS,
        ),
    ]


def next_evidence_steps(release_missing: list[dict[str, Any]]) -> list[dict[str, Any]]:
    missing_by_gate = {
        str(gate.get("id")): [str(item) for item in gate.get("missing", [])]
        for gate in release_missing
    }
    steps: list[dict[str, Any]] = []
    viewer_missing = missing_by_gate.get("viewer_desktop_matrix", [])
    if viewer_missing:
        viewer_commands: list[dict[str, Any]] = [
            {
                "label": "Export source bundle for external viewer rows",
                "command": "scripts/export_release_evidence_bundle.py",
                    "notes": [
                        "Use this before running viewer matrix probes on another desktop or machine.",
                        "Extract the tarball there and run ./collect-viewer-evidence.sh from the bundle root.",
                        "Use only the extracted repo-owned runtime collector; do not substitute Codex app MCP, Computer Use MCP, Playwright MCP, or Codex Desktop bridge behavior as viewer evidence.",
                        "The bundled collector verifies copied source bytes before stamping the bundle manifest source identity.",
                        "The generated viewer report must include evidence_boundary showing repo-owned runtime collection.",
                        "Copy the generated JSON report back and import it on the release machine.",
                    ],
            },
            {
                "label": "Import copied viewer reports on the release machine",
                "command": "scripts/import_release_evidence.py /path/to/copied/viewer-report-or-directory",
                "notes": [
                    "Run this after copying JSON reports back from another desktop/session.",
                    "The importer rejects source-hash mismatches, skipped/failed rows, contradictory session attestation, missing repo-owned evidence_boundary, and non-release display attestation by default.",
                ],
            },
        ]
        missing_kde = any("KDE" in item or "Plasma" in item for item in viewer_missing)
        missing_x11 = any("X11" in item for item in viewer_missing)
        if missing_kde or missing_x11:
            if missing_kde and missing_x11:
                label = "KDE/Plasma or X11 row"
                scope_note = (
                    "Run this from a real KDE/Plasma session and from a real X11 session until the audit no longer reports those rows missing."
                )
            elif missing_kde:
                label = "KDE/Plasma viewer row"
                scope_note = "Run this from a real KDE/Plasma session."
            else:
                label = "X11 viewer row"
                scope_note = "Run this from a real X11 session."
            viewer_commands.insert(
                1,
                {
                    "label": label,
                    "command": "REQUIRE_VIEWER_SMOKE=1 scripts/viewer_desktop_matrix_probe.sh",
                    "notes": [
                        scope_note,
                        "Do not override XDG_SESSION_TYPE or XDG_CURRENT_DESKTOP by hand; the report must reflect the actual session.",
                        "When loginctl metadata is available, contradictory session-type claims are marked not release eligible.",
                        "Display attestation must include an existing socket and display-server process for the reported session; remote X forwarding and nested/headless host displays such as Xvfb, xpra, Xephyr, and headless Weston do not count.",
                    ],
                },
            )
        if any("native Wayland" in item for item in viewer_missing):
            viewer_commands.append(
                {
                    "label": "Native Wayland compositor observation",
                    "command": (
                        "REQUIRE_VIEWER_SMOKE=1 "
                        "NATIVE_WAYLAND_LAYER_SHELL_OBSERVED=1 "
                        "NATIVE_WAYLAND_LAYER_SHELL_NOTES='<compositor, desktop, observed layer-shell/top-layer behavior>' "
                        "scripts/viewer_desktop_matrix_probe.sh"
                    ),
                    "notes": [
                        "Run only from an actual Wayland session after compositor-level observation.",
                        "The notes must make a positive layer-shell/top-layer claim for the viewer.",
                        "The collector, audit, and importer reject X11 sessions, missing notes, GNOME/Xwayland fallback notes, forced X11/Xwayland viewer backends, and notes that say the viewer was not layer-shell.",
                    ],
                }
            )
        steps.append(
            {
                "id": "viewer_desktop_matrix",
                "title": "Collect missing Linux desktop viewer rows",
                "why": "Release evidence must cover modern Linux desktops rather than only this local session.",
                "commands": viewer_commands,
            }
        )

    app_qa_missing = missing_by_gate.get("app_qa_dogfood", [])
    if app_qa_missing:
        steps.append(
            {
                "id": "app_qa_dogfood",
                "title": "Run local app-QA dogfood evidence",
                "why": "Planning tests are not enough; release evidence should prove a GUI app can be launched, observed, logged, and stopped.",
                "commands": [
                    {
                        "label": "Collect local app-QA dogfood",
                        "command": "scripts/app_qa_dogfood_smoke.sh",
                        "notes": [
                            "Requires a local X11-capable GUI smoke environment with xmessage.",
                            "Writes a JSON report under target/app-qa-dogfood/ for the current combined source identity.",
                            "The report includes evidence_boundary showing repo-owned runtime collection without Codex app MCP, Computer Use MCP, Playwright MCP, or Codex Desktop bridge evidence.",
                            "The release audit rejects stale reports, source mismatches, missing screenshots, missing event/log evidence, or host/real-world mutation drift.",
                        ],
                    }
                ],
            }
        )

    github_missing = missing_by_gate.get("github_explore_dogfood", [])
    if github_missing:
        steps.append(
            {
                "id": "github_explore_dogfood",
                "title": "Run visible GitHub Explore dogfood",
                "why": "Release evidence should prove real workspace browser discovery while the user can see the GPUI viewer.",
                "commands": [
                    {
                        "label": "Collect GitHub Explore repository-discovery dogfood",
                        "command": "scripts/github_explore_dogfood_probe.js",
                        "notes": [
                            "By default this opens the GPUI viewer immediately through workspace_open_viewer and keeps it always on top.",
                            "Use GITHUB_EXPLORE_OPEN_VIEWER=0 only for explicit no-viewer automation.",
                            "The report is rejected unless it includes workspace_open_viewer launch metadata, workspace-owned Chrome DevTools evidence, a screenshot, events, clean stop, and at least three GitHub repository recommendations.",
                            "Do not substitute host Chrome, Codex app MCP, Computer Use MCP, curl, or Playwright evidence.",
                        ],
                    },
                ],
            }
        )

    human_missing = missing_by_gate.get("human_final_diff_review", [])
    if human_missing:
        steps.append(
            {
                "id": "human_final_diff_review",
                "title": "Complete human review and strict release audit",
                "why": "Local automation cannot prove that a human accepted the large runtime and Desktop diffs.",
                "commands": [
                    {
                        "label": "Export source bundle for off-machine human review",
                        "command": "scripts/export_release_evidence_bundle.py",
                        "notes": [
                            "Use this when final human review or marker creation happens on another machine.",
                            "Extract the tarball there before running ./create-human-review-marker.sh from the bundle root.",
                            "The extracted marker helper verifies copied source bytes before stamping the bundle manifest source and review-scope identity.",
                        ],
                    },
                    {
                        "label": "Create human review marker after review",
                        "command": (
                            "scripts/create_human_review_marker.py "
                            "--reviewer \"$USER\" "
                            "--confirm-reviewed "
                            "--notes \"$HUMAN_REVIEW_NOTES\""
                        ),
                        "notes": [
                            "Run only after a human has inspected the generated runtime and sibling Desktop review artifacts.",
                            "Set HUMAN_REVIEW_NOTES to specific non-placeholder notes about what was reviewed and accepted.",
                            "The command regenerates review artifacts from the current source/review scope and writes target/release-gate-human-review.json.",
                            "Use --replace only after reviewing the regenerated artifacts for the new source scope.",
                        ],
                    },
                    {
                        "label": "Create human review marker from extracted source bundle",
                        "command": (
                            "./create-human-review-marker.sh "
                            "--reviewer \"$USER\" "
                            "--confirm-reviewed "
                            "--notes \"$HUMAN_REVIEW_NOTES\""
                        ),
                        "notes": [
                            "Run this from an extracted release evidence bundle after human review there.",
                            "Set HUMAN_REVIEW_NOTES to specific non-placeholder notes before creating the marker.",
                            "Copy the marker JSON and generated runtime/Desktop review artifact files back together.",
                            "Do not edit the extracted source before creating the marker; the helper verifies copied source bytes against the bundle manifest and the importer verifies source/review-scope identity.",
                        ],
                    },
                    {
                        "label": "Import copied human review marker and artifacts",
                        "command": "scripts/import_release_evidence.py /path/to/copied/human-review-marker-or-directory",
                        "notes": [
                            "Use this when the review marker was created on another machine from the same source/review scope.",
                            "Copy the marker JSON and the generated runtime/Desktop review artifact files together.",
                            "The importer rewrites artifact paths to local target/final-review-bundle files and rejects missing bytes, source mismatches, and review-scope mismatches.",
                        ],
                    },
                    {
                        "label": "Final strict release gate after evidence and marker",
                        "command": "REQUIRE_RELEASE_GATES=1 scripts/prod_readiness_smoke.sh",
                        "notes": [
                            "Run after collecting missing external evidence, completing human review, creating the marker, and cleaning or intentionally staging source changes.",
                            "Strict mode requires all release gates, a current review-scope marker, and a clean runtime and Desktop source identity.",
                        ],
                    }
                ],
                "manual_followup": [
                    "Do not create the marker before review; the guarded marker script requires explicit --confirm-reviewed.",
                ],
            }
        )
    return steps


def release_gate_consistency(
    source_identity: dict[str, Any],
    review_scope_identity: dict[str, Any],
    evidence: dict[str, Any],
) -> dict[str, Any]:
    release_entry = evidence.get("release_gate_audit") or {}
    release_gate = release_entry.get("report") or {}
    inputs = release_gate.get("inputs") or {}
    audit_source = inputs.get("source_identity") or {}
    audit_review_scope = inputs.get("review_scope_identity") or {}
    current_hash = source_identity.get("source_hash")
    audit_hash = audit_source.get("source_hash")
    current_head = source_identity.get("git_head")
    audit_head = audit_source.get("git_head")
    current_review_scope_hash = review_scope_identity.get("review_scope_hash")
    audit_review_scope_hash = audit_review_scope.get("review_scope_hash")
    current_review_scope_head = review_scope_identity.get("git_head")
    audit_review_scope_head = audit_review_scope.get("git_head")
    return {
        "release_gate_audit_path": release_entry.get("path"),
        "release_gate_status": release_gate.get("status"),
        "release_gate_created_at_utc": release_gate.get("created_at_utc"),
        "current_source_hash": current_hash,
        "release_gate_source_hash": audit_hash,
        "current_git_head": current_head,
        "release_gate_git_head": audit_head,
        "current_components": source_identity.get("components"),
        "release_gate_components": audit_source.get("components"),
        "current_review_scope_hash": current_review_scope_hash,
        "release_gate_review_scope_hash": audit_review_scope_hash,
        "current_review_scope_git_head": current_review_scope_head,
        "release_gate_review_scope_git_head": audit_review_scope_head,
        "matches_current_source": bool(
            current_hash and audit_hash and current_hash == audit_hash and current_head == audit_head
        ),
        "matches_current_review_scope": bool(
            current_review_scope_hash
            and audit_review_scope_hash
            and current_review_scope_hash == audit_review_scope_hash
            and current_review_scope_head == audit_review_scope_head
        ),
    }


def build_bundle(desktop_repo: Path) -> dict[str, Any]:
    source_identity = compute_source_identity(ROOT, desktop_repo=desktop_repo)
    review_scope_identity = compute_review_scope_identity(ROOT, desktop_repo=desktop_repo)
    runtime_identity = compute_runtime_source_identity(ROOT)
    desktop_identity = compute_desktop_source_identity(desktop_repo)
    evidence = latest_evidence()
    release_gate = evidence.get("release_gate_audit", {}).get("report") or {}
    release_missing = [
        {
            "id": gate.get("id"),
            "missing": gate.get("missing", []),
        }
        for gate in release_gate.get("gates", [])
        if gate.get("status") != "passed"
    ]
    return {
        "schema": "agent-workspace-linux.final_human_review_bundle.v1",
        "created_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "source_identity": source_identity,
        "review_scope_identity": review_scope_identity,
        "runtime_repo": repo_snapshot(
            ROOT,
            source_paths=RUNTIME_SOURCE_IDENTITY_PATHS,
            source_label="runtime",
        ),
        "desktop_repo": repo_snapshot(
            desktop_repo,
            source_paths=DESKTOP_SOURCE_IDENTITY_PATHS,
            source_label="codex_desktop",
        ),
        "runtime_source_identity": runtime_identity,
        "desktop_source_identity": desktop_identity,
        "latest_evidence": evidence,
        "release_gate_status": release_gate.get("status"),
        "release_gate_consistency": release_gate_consistency(
            source_identity,
            review_scope_identity,
            evidence,
        ),
        "release_gate_missing": release_missing,
        "next_evidence_steps": next_evidence_steps(release_missing),
        "human_review_marker_path": str(DEFAULT_HUMAN_REVIEW_MARKER),
        "human_review_marker_template": marker_template(
            source_identity,
            review_scope_identity,
        ),
        "review_checklist": [
            "Confirm the runtime diff scope matches the MCP permission, planning, dogfood, viewer, and release-gate goal.",
            "Confirm the sibling Desktop diff remains a thin integration layer and does not duplicate the main GPUI viewer surface.",
            "Review the generated runtime and sibling Desktop diff artifacts and preserve their hashes in the marker template.",
            "Confirm generated lockfile and dependency changes are intentional.",
            "Confirm handover/audit docs should stay tracked or move to release artifacts.",
            "Run strict release evidence collection before creating the human-review marker.",
        ],
    }


def write_markdown(bundle: dict[str, Any], path: Path) -> None:
    runtime = bundle["runtime_repo"]
    desktop = bundle["desktop_repo"]
    combined = bundle.get("source_identity") or {}
    review_scope = bundle.get("review_scope_identity") or {}
    review_scope_modes = ", ".join(
        f"{name}:{component.get('review_mode')}"
        for name, component in (review_scope.get("components") or {}).items()
    ) or "unknown"
    release_missing = bundle["release_gate_missing"]
    evidence = bundle.get("latest_evidence") or {}
    source_bundle = evidence.get("release_evidence_source_bundle") or {}
    lines = [
        "# Final Human Review Bundle",
        "",
        f"Created: {bundle['created_at_utc']}",
        "",
        "## Combined Source Identity",
        "",
        f"- Source hash: `{combined.get('source_hash')}`",
        f"- Git heads: `{combined.get('git_head')}`",
        f"- Dirty entries: `{combined.get('source_dirty_count')}`",
        f"- Missing components: `{', '.join(combined.get('missing_components') or []) or 'none'}`",
        f"- Review scope hash: `{review_scope.get('review_scope_hash')}`",
        f"- Review scope dirty entries: `{review_scope.get('dirty_count')}`",
        f"- Review scope modes: `{review_scope_modes}`",
        "",
        "## Runtime Repo",
        "",
        f"- Path: `{runtime['path']}`",
        f"- Head: `{runtime.get('git_head')}`",
        f"- Dirty entries: `{runtime.get('dirty_count')}`",
        f"- Source hash: `{runtime.get('source_identity', {}).get('source_hash')}`",
        "",
        "## Desktop Repo",
        "",
        f"- Path: `{desktop['path']}`",
        f"- Exists: `{desktop.get('exists')}`",
        f"- Head: `{desktop.get('git_head')}`",
        f"- Dirty entries: `{desktop.get('dirty_count')}`",
        f"- Source hash: `{desktop.get('source_identity', {}).get('source_hash')}`",
        "",
        "## Release Gate Consistency",
        "",
        f"- Audit path: `{bundle.get('release_gate_consistency', {}).get('release_gate_audit_path')}`",
        f"- Audit status: `{bundle.get('release_gate_consistency', {}).get('release_gate_status')}`",
        f"- Current source hash: `{bundle.get('release_gate_consistency', {}).get('current_source_hash')}`",
        f"- Audit source hash: `{bundle.get('release_gate_consistency', {}).get('release_gate_source_hash')}`",
        f"- Matches current source: `{bundle.get('release_gate_consistency', {}).get('matches_current_source')}`",
        f"- Current review scope hash: `{bundle.get('release_gate_consistency', {}).get('current_review_scope_hash')}`",
        f"- Audit review scope hash: `{bundle.get('release_gate_consistency', {}).get('release_gate_review_scope_hash')}`",
        f"- Matches current review scope: `{bundle.get('release_gate_consistency', {}).get('matches_current_review_scope')}`",
        "",
        "## Latest Evidence Artifacts",
        "",
        f"- App-QA dogfood report: `{(evidence.get('app_qa_dogfood') or {}).get('path')}`",
        f"- GitHub Explore dogfood report: `{(evidence.get('github_explore_dogfood') or {}).get('path')}`",
        f"- Viewer desktop matrix report: `{(evidence.get('viewer_desktop_matrix') or {}).get('path')}`",
        f"- Release gate audit: `{(evidence.get('release_gate_audit') or {}).get('path')}`",
        f"- External viewer source bundle: `{source_bundle.get('path')}`",
        f"- Source bundle hash: `{((source_bundle.get('manifest') or {}).get('source_identity') or {}).get('source_hash')}`",
        "",
        "## Review Artifacts",
        "",
    ]
    review_artifacts = bundle.get("review_artifacts") or []
    if review_artifacts:
        for artifact in review_artifacts:
            lines.extend(
                [
                    f"- `{artifact.get('label')}`: `{artifact.get('path')}`",
                    f"  - SHA256: `{artifact.get('sha256')}`",
                    f"  - Size: `{artifact.get('size_bytes')}` bytes",
                ]
            )
    else:
        lines.append("- None generated.")
    lines.extend(
        [
            "",
            "## Pending Release Gates",
            "",
        ]
    )
    if release_missing:
        for gate in release_missing:
            lines.append(f"- `{gate['id']}`: {', '.join(gate.get('missing') or [])}")
    else:
        lines.append("- None in the latest release-gate audit.")
    lines.extend(["", "## Next Evidence Commands", ""])
    next_steps = bundle.get("next_evidence_steps") or []
    if next_steps:
        for step in next_steps:
            lines.extend(
                [
                    f"### {step['title']}",
                    "",
                    step["why"],
                    "",
                ]
            )
            for command in step.get("commands", []):
                lines.extend(
                    [
                        f"- {command['label']}:",
                        "",
                        "```bash",
                        command["command"],
                        "```",
                        "",
                    ]
                )
                for note in command.get("notes", []):
                    lines.append(f"  - {note}")
                if command.get("notes"):
                    lines.append("")
            for followup in step.get("manual_followup", []):
                lines.append(f"- {followup}")
            if step.get("manual_followup"):
                lines.append("")
    else:
        lines.append("- No additional evidence commands are needed by the latest audit.")
    lines.extend(
        [
            "",
            "## Review Checklist",
            "",
            *[f"- {item}" for item in bundle["review_checklist"]],
            "",
            "## Human Review Marker Template",
            "",
            "Do not create this marker until a human has actually reviewed the diff.",
            "",
            "```json",
            json.dumps(bundle["human_review_marker_template"], indent=2, sort_keys=True),
            "```",
            "",
        ]
    )
    path.write_text("\n".join(lines), encoding="utf-8")


def assert_self_test(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(f"final review bundle self-test failed: {message}")


def run_self_test() -> None:
    release_missing = [
        {
            "id": "viewer_desktop_matrix",
            "missing": ["KDE/Plasma viewer smoke row", "X11 viewer smoke row"],
        },
        {
            "id": "app_qa_dogfood",
            "missing": ["local GUI app-QA dogfood report"],
        },
        {
            "id": "github_explore_dogfood",
            "missing": ["GitHub Explore dogfood report"],
        },
        {
            "id": "human_final_diff_review",
            "missing": ["human review marker"],
        },
    ]
    steps = next_evidence_steps(release_missing)
    assert_self_test(
        [step["id"] for step in steps]
        == [
            "viewer_desktop_matrix",
            "app_qa_dogfood",
            "github_explore_dogfood",
            "human_final_diff_review",
        ],
        "next evidence steps should track pending gates in order",
    )
    command_text = "\n".join(
        command["command"] for step in steps for command in step.get("commands", [])
    )
    note_text = "\n".join(
        note
        for step in steps
        for command in step.get("commands", [])
        for note in command.get("notes", [])
    )
    assert_self_test(
        "NATIVE_WAYLAND_LAYER_SHELL_NOTES" not in command_text,
        "native Wayland command should not appear after that row is already satisfied",
    )
    native_steps = next_evidence_steps(
        [
            {
                "id": "viewer_desktop_matrix",
                "missing": ["native Wayland layer-shell/compositor observation with notes"],
            }
        ]
    )
    native_command_text = "\n".join(
        command["command"] for step in native_steps for command in step.get("commands", [])
    )
    assert_self_test(
        "NATIVE_WAYLAND_LAYER_SHELL_NOTES" in native_command_text,
        "native Wayland command should require observation notes when that row is missing",
    )
    native_notes = "\n".join(
        note
        for step in native_steps
        for command in step.get("commands", [])
        for note in command.get("notes", [])
    )
    assert_self_test(
        "positive layer-shell/top-layer claim" in native_notes
        and "GNOME/Xwayland fallback notes" in native_notes,
        "native Wayland instructions should describe the stricter release evidence rule",
    )
    kde_only_steps = next_evidence_steps(
        [
            {
                "id": "viewer_desktop_matrix",
                "missing": [
                    "KDE/Plasma viewer smoke row",
                    "native Wayland layer-shell/compositor observation with notes",
                ],
            }
        ]
    )
    kde_only_label_text = "\n".join(
        str(command.get("label") or "")
        for step in kde_only_steps
        for command in step.get("commands", [])
    )
    kde_only_note_text = "\n".join(
        note
        for step in kde_only_steps
        for command in step.get("commands", [])
        for note in command.get("notes", [])
    )
    assert_self_test(
        "KDE/Plasma viewer row" in kde_only_label_text
        and "real X11 session" not in kde_only_note_text,
        "KDE-only viewer instructions should not keep asking for an already-satisfied X11 row",
    )
    assert_self_test(
        "scripts/github_explore_dogfood_probe.js" in command_text,
        "GitHub Explore next steps should include the visible repository-discovery collector",
    )
    assert_self_test(
        "workspace_open_viewer" in note_text
        and "launch metadata" in note_text
        and "workspace-owned Chrome DevTools" in note_text,
        "GitHub Explore next steps should require viewer metadata and workspace-owned browser proof",
    )
    assert_self_test(
        "GITHUB_EXPLORE_OPEN_VIEWER=0" in note_text,
        "GitHub Explore next steps should make no-viewer mode an explicit opt-out",
    )
    assert_self_test(
        "scripts/app_qa_dogfood_smoke.sh" in command_text,
        "app-QA next steps should include the local GUI dogfood collector",
    )
    assert_self_test(
        "collect_real_grocery_evidence.sh" not in command_text,
        "real grocery commands should not be part of the default next evidence plan",
    )
    assert_self_test(
        "scripts/import_release_evidence.py" in command_text,
        "next evidence commands should include external evidence import",
    )
    assert_self_test(
        "scripts/create_human_review_marker.py" in command_text,
        "human-review next steps should include the guarded marker generator",
    )
    assert_self_test(
        "./create-human-review-marker.sh" in command_text,
        "human-review next steps should include the extracted bundle marker helper",
    )
    assert_self_test(
        "human-review-marker-or-directory" in command_text,
        "human-review next steps should include guarded marker import",
    )
    assert_self_test(
        "--confirm-reviewed" in command_text,
        "human-review marker command should require explicit review confirmation",
    )
    assert_self_test(
        'HUMAN_REVIEW_NOTES' in command_text,
        "human-review marker command should require explicit non-placeholder notes",
    )
    assert_self_test(
        "scripts/export_release_evidence_bundle.py" in command_text,
        "next evidence commands should include source bundle export",
    )
    human_only_steps = next_evidence_steps(
        [{"id": "human_final_diff_review", "missing": ["human review marker"]}]
    )
    human_only_command_text = "\n".join(
        command["command"] for step in human_only_steps for command in step.get("commands", [])
    )
    assert_self_test(
        "scripts/export_release_evidence_bundle.py" in human_only_command_text
        and "./create-human-review-marker.sh" in human_only_command_text,
        "human-only next steps should still include export and extracted marker commands",
    )
    consistency = release_gate_consistency(
        {"source_hash": "current", "git_head": "head"},
        {"review_scope_hash": "scope-current", "git_head": "head"},
        {
            "release_gate_audit": {
                "path": "audit.json",
                "report": {
                    "inputs": {
                        "source_identity": {"source_hash": "old", "git_head": "head"},
                        "review_scope_identity": {
                            "review_scope_hash": "scope-old",
                            "git_head": "head",
                        },
                    }
                },
            }
        },
    )
    assert_self_test(
        consistency["matches_current_source"] is False,
        "consistency check should detect stale release gate source hash",
    )
    assert_self_test(
        consistency["matches_current_review_scope"] is False,
        "consistency check should detect stale review scope hash",
    )
    with tempfile.TemporaryDirectory(prefix="agent-workspace-final-bundle-self-test-") as temp:
        root = Path(temp) / "bundle-root"
        root.mkdir()
        manifest = {
            "schema": "agent-workspace-linux.release_evidence_source_bundle.v1",
            "source_identity": {"source_hash": "source"},
        }
        (root / SOURCE_BUNDLE_MANIFEST).write_text(json.dumps(manifest), encoding="utf-8")
        tar_path = Path(temp) / "bundle.tar.gz"
        with tarfile.open(tar_path, "w:gz") as archive:
            archive.add(root, arcname="bundle-root")
        assert_self_test(
            read_source_bundle_manifest(tar_path)["source_identity"]["source_hash"] == "source",
            "source bundle manifest should be readable from tarball",
        )
        digest_path = Path(temp) / "digest.txt"
        digest_path.write_text("review me\n", encoding="utf-8")
        assert_self_test(
            file_sha256(digest_path)
            == "0326ce3de6e46a892bc36c7877d5d187af0508ddc92efef75ec75577dc7ca48a",
            "file sha256 helper should hash review artifacts deterministically",
        )
        snapshot_root = Path(temp) / "nogit-runtime"
        (snapshot_root / "src").mkdir(parents=True)
        (snapshot_root / "scripts").mkdir()
        (snapshot_root / "src" / "main.rs").write_text(
            "fn main() { println!(\"review\"); }\n",
            encoding="utf-8",
        )
        (snapshot_root / "scripts" / "smoke.sh").write_text(
            "#!/usr/bin/env bash\ntrue\n",
            encoding="utf-8",
        )
        snapshot_artifact = Path(temp) / "nogit-review.diff"
        write_repo_review_diff(
            snapshot_root,
            snapshot_artifact,
            label="nogit_runtime",
            source_paths=["src", "scripts"],
        )
        snapshot_text = snapshot_artifact.read_text(encoding="utf-8")
        assert_self_test(
            "No Git Repository Source Inventory" in snapshot_text
            and "src/main.rs" in snapshot_text
            and "Source File Patch: src/main.rs" in snapshot_text,
            "no-git review artifacts should include source inventory and patch views",
        )
    print("final review bundle self-test passed")


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        run_self_test()
        return 0
    output_dir = Path(os.environ.get("FINAL_REVIEW_BUNDLE_DIR", DEFAULT_OUTPUT_DIR))
    desktop_repo = Path(os.environ.get("CODEX_DESKTOP_LINUX_REPO", DEFAULT_DESKTOP_REPO))
    output_dir.mkdir(parents=True, exist_ok=True)
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    bundle = build_bundle(desktop_repo)
    review_artifacts = write_review_artifacts(
        output_dir,
        stamp,
        desktop_repo=desktop_repo,
    )
    bundle["review_artifacts"] = review_artifacts
    bundle["human_review_marker_template"] = marker_template(
        bundle["source_identity"],
        bundle["review_scope_identity"],
        review_artifacts,
    )
    json_path = output_dir / f"{stamp}.json"
    md_path = output_dir / f"{stamp}.md"
    json_path.write_text(json.dumps(bundle, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    write_markdown(bundle, md_path)
    print(f"final review bundle json: {json_path}")
    print(f"final review bundle markdown: {md_path}")
    print(f"release gate status: {bundle.get('release_gate_status')}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
