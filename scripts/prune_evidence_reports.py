#!/usr/bin/env python3
"""Prune timestamped local evidence reports while keeping recent runs grouped."""

from __future__ import annotations

import argparse
import json
import re
import shutil
import tempfile
from dataclasses import dataclass
from pathlib import Path

from release_gate_audit import native_wayland_observed


ROOT = Path(__file__).resolve().parent.parent
TIMESTAMP_PREFIX_RE = re.compile(r"^(\d{8}T\d{6}Z)")
DEFAULT_REPORT_DIRS = [
    ROOT / "target" / "app-qa-dogfood",
    ROOT / "target" / "final-review-bundle",
    ROOT / "target" / "objective-completion-audit",
    ROOT / "target" / "prod-readiness-smoke",
    ROOT / "target" / "github-explore-dogfood",
    ROOT / "target" / "real-grocery-dogfood",
    ROOT / "target" / "real-grocery-preflight",
    ROOT / "target" / "release-evidence-source-bundle",
    ROOT / "target" / "release-gate-audit",
    ROOT / "target" / "viewer-desktop-matrix",
]


@dataclass(frozen=True)
class EvidenceGroup:
    stamp: str
    files: tuple[Path, ...]


def timestamp_prefix(path: Path) -> str | None:
    match = TIMESTAMP_PREFIX_RE.match(path.name)
    return match.group(1) if match else None


def evidence_groups(directory: Path) -> list[EvidenceGroup]:
    grouped: dict[str, list[Path]] = {}
    if not directory.exists():
        return []
    for path in directory.iterdir():
        if not path.is_file():
            continue
        stamp = timestamp_prefix(path)
        if stamp is None:
            continue
        grouped.setdefault(stamp, []).append(path)
    return [
        EvidenceGroup(stamp=stamp, files=tuple(sorted(files)))
        for stamp, files in sorted(grouped.items())
    ]


def read_json(path: Path) -> dict[str, object] | None:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None
    return value if isinstance(value, dict) else None


def text(value: object) -> str:
    return str(value or "").strip().lower()


def viewer_session_type(report: dict[str, object]) -> str:
    session = report.get("session") if isinstance(report.get("session"), dict) else {}
    loginctl = session.get("loginctl") if isinstance(session.get("loginctl"), dict) else {}
    matrix = report.get("matrix_result") if isinstance(report.get("matrix_result"), dict) else {}
    return text(
        session.get("xdg_session_type")
        or loginctl.get("Type")
        or matrix.get("session_type")
    )


def viewer_desktop_label(report: dict[str, object]) -> str:
    matrix = report.get("matrix_result") if isinstance(report.get("matrix_result"), dict) else {}
    session = report.get("session") if isinstance(report.get("session"), dict) else {}
    return text(matrix.get("desktop_label") or session.get("xdg_current_desktop"))


def protected_report_reason(report: dict[str, object]) -> str | None:
    schema = report.get("schema")
    if schema == "agent-workspace-linux.viewer_desktop_matrix.v1":
        smoke = report.get("viewer_smoke") if isinstance(report.get("viewer_smoke"), dict) else {}
        matrix = report.get("matrix_result") if isinstance(report.get("matrix_result"), dict) else {}
        consistency = (
            matrix.get("session_consistency")
            if isinstance(matrix.get("session_consistency"), dict)
            else {}
        )
        if (
            smoke.get("status") != "passed"
            or matrix.get("counts_for_release_matrix") is not True
            or consistency.get("release_eligible") is False
        ):
            return None
        desktop = viewer_desktop_label(report)
        session_type = viewer_session_type(report)
        if native_wayland_observed(report):
            return "native Wayland compositor observation"
        if session_type == "x11":
            return "X11 viewer matrix row"
        if "kde" in desktop or "plasma" in desktop:
            return "KDE/Plasma viewer matrix row"
        return None
    if schema == "agent-workspace-linux.real_grocery_dogfood_probe.v1":
        real_browser = (
            report.get("real_browser") if isinstance(report.get("real_browser"), dict) else {}
        )
        if report.get("mode") == "real-browser" and real_browser.get("status") == "passed":
            return "real-browser grocery dogfood evidence"
    if schema == "agent-workspace-linux.github_explore_dogfood.v1":
        if report.get("mode") == "workspace-github-explore" and report.get("status") == "passed":
            return "GitHub Explore dogfood evidence"
    return None


def group_protection_reason(group: EvidenceGroup) -> str | None:
    for path in group.files:
        if path.suffix != ".json":
            continue
        report = read_json(path)
        if report is None:
            continue
        reason = protected_report_reason(report)
        if reason is not None:
            return reason
    return None


def prune_directory(directory: Path, *, keep: int, dry_run: bool) -> dict[str, object]:
    groups = evidence_groups(directory)
    old_groups = groups[: max(0, len(groups) - keep)]
    protected = {group.stamp: group_protection_reason(group) for group in old_groups}
    removable = [group for group in old_groups if protected[group.stamp] is None]
    removed_files: list[str] = []
    for group in removable:
        for path in group.files:
            removed_files.append(str(path))
            if not dry_run:
                path.unlink(missing_ok=True)
    return {
        "directory": str(directory),
        "groups_before": len(groups),
        "groups_kept": len(groups) - len(removable),
        "groups_removed": len(removable),
        "groups_protected": len([reason for reason in protected.values() if reason is not None]),
        "protected_groups": [
            {"stamp": stamp, "reason": reason}
            for stamp, reason in protected.items()
            if reason is not None
        ],
        "files_removed": removed_files,
        "dry_run": dry_run,
    }


def prune_reports(directories: list[Path], *, keep: int, dry_run: bool) -> dict[str, object]:
    if keep < 1:
        raise ValueError("--keep must be at least 1")
    results = [prune_directory(directory, keep=keep, dry_run=dry_run) for directory in directories]
    return {
        "schema": "agent-workspace-linux.evidence_retention.v1",
        "keep": keep,
        "dry_run": dry_run,
        "directories": results,
        "files_removed_count": sum(len(item["files_removed"]) for item in results),
        "groups_removed_count": sum(int(item["groups_removed"]) for item in results),
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Prune timestamped generated evidence under target/ while keeping recent grouped runs."
    )
    parser.add_argument("--keep", type=int, default=25, help="timestamp groups to keep per directory")
    parser.add_argument("--dry-run", action="store_true", help="show what would be removed")
    parser.add_argument(
        "--dir",
        action="append",
        dest="dirs",
        type=Path,
        help="directory to prune; may be repeated. Defaults to known target evidence dirs.",
    )
    parser.add_argument("--self-test", action="store_true", help="run synthetic retention checks")
    return parser.parse_args()


def assert_self_test(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def write_group(directory: Path, stamp: str, suffixes: list[str]) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    for suffix in suffixes:
        (directory / f"{stamp}{suffix}").write_text(f"{stamp}{suffix}\n", encoding="utf-8")


def write_json_group(directory: Path, stamp: str, value: dict[str, object]) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    (directory / f"{stamp}.json").write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    (directory / f"{stamp}-gpui-viewer-smoke.log").write_text("smoke log\n", encoding="utf-8")


def run_self_test() -> None:
    temp = Path(tempfile.mkdtemp(prefix="agent-workspace-report-retention-"))
    try:
        grouped = temp / "final-review-bundle"
        for index in range(5):
            stamp = f"20260525T12000{index}Z"
            write_group(
                grouped,
                stamp,
                [".json", ".md", "-runtime-review.diff", "-codex-desktop-review.diff"],
            )
        (grouped / "release-gate-human-review.json").write_text("keep me\n", encoding="utf-8")

        dry_report = prune_reports([grouped], keep=2, dry_run=True)
        assert_self_test(dry_report["files_removed_count"] == 12, "dry-run should count old grouped files")
        assert_self_test(len(list(grouped.iterdir())) == 21, "dry-run must not delete files")

        report = prune_reports([grouped], keep=2, dry_run=False)
        remaining = sorted(path.name for path in grouped.iterdir())
        assert_self_test(report["groups_removed_count"] == 3, "three old groups should be removed")
        assert_self_test(
            "release-gate-human-review.json" in remaining,
            "non-timestamped marker files must be preserved",
        )
        assert_self_test(
            all(name.startswith(("20260525T120003Z", "20260525T120004Z", "release-gate")) for name in remaining),
            "only the newest two timestamp groups should remain",
        )

        try:
            prune_reports([grouped], keep=0, dry_run=False)
        except ValueError:
            pass
        else:
            raise AssertionError("keep=0 should be rejected by the pruning script")

        viewer = temp / "viewer-desktop-matrix"
        for index in range(4):
            native_positive = index == 0
            native_negative = index == 1
            write_json_group(
                viewer,
                f"20260525T13000{index}Z",
                {
                    "schema": "agent-workspace-linux.viewer_desktop_matrix.v1",
                    "viewer_smoke": {"status": "passed"},
                    "matrix_result": {
                        "counts_for_release_matrix": True,
                        "desktop_label": (
                            "KDE / wayland"
                            if native_positive
                            else "ubuntu:GNOME / wayland / ubuntu"
                        ),
                        "session_consistency": {"release_eligible": True},
                        "native_wayland_layer_shell_observed": native_positive
                        or native_negative,
                        "native_wayland_layer_shell_notes": (
                            "Observed layer-shell top-layer behavior on KWin Wayland."
                            if native_positive
                            else (
                                "Observed a normal Xwayland toplevel, not layer-shell."
                                if native_negative
                                else None
                            )
                        ),
                    },
                    "session": {
                        "xdg_session_type": "wayland",
                        "xdg_current_desktop": "KDE" if native_positive else "GNOME",
                        "desktop_session": "plasma" if native_positive else "gnome",
                    },
                },
            )
        viewer_report = prune_reports([viewer], keep=2, dry_run=False)
        viewer_remaining = sorted(path.name for path in viewer.iterdir())
        assert_self_test(
            viewer_report["directories"][0]["groups_protected"] == 1,
            "rare native Wayland observation should be protected from pruning",
        )
        assert_self_test(
            any(name.startswith("20260525T130000Z") for name in viewer_remaining),
            "protected native Wayland evidence should remain even outside retention window",
        )
        assert_self_test(
            not any(name.startswith("20260525T130001Z") for name in viewer_remaining),
            "GNOME/Xwayland negative native notes should still be pruned",
        )

        github = temp / "github-explore-dogfood"
        write_json_group(
            github,
            "20260525T140000Z",
            {
                "schema": "agent-workspace-linux.github_explore_dogfood.v1",
                "mode": "workspace-github-explore",
                "status": "passed",
            },
        )
        write_json_group(
            github,
            "20260525T140001Z",
            {
                "schema": "agent-workspace-linux.github_explore_dogfood.v1",
                "mode": "workspace-github-explore",
                "status": "failed",
            },
        )
        write_json_group(
            github,
            "20260525T140002Z",
            {
                "schema": "agent-workspace-linux.github_explore_dogfood.v1",
                "mode": "workspace-github-explore",
                "status": "failed",
            },
        )
        github_report = prune_reports([github], keep=1, dry_run=False)
        github_remaining = sorted(path.name for path in github.iterdir())
        assert_self_test(
            github_report["directories"][0]["groups_protected"] == 1,
            "GitHub Explore dogfood evidence should be protected from pruning",
        )
        assert_self_test(
            any(name.startswith("20260525T140000Z") for name in github_remaining),
            "protected GitHub Explore dogfood evidence should remain",
        )
    finally:
        shutil.rmtree(temp, ignore_errors=True)
    print("evidence report retention self-test passed")


def main() -> int:
    args = parse_args()
    if args.self_test:
        run_self_test()
        return 0
    directories = [path.resolve() for path in (args.dirs or DEFAULT_REPORT_DIRS)]
    report = prune_reports(directories, keep=args.keep, dry_run=args.dry_run)
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
