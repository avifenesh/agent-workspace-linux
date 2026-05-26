#!/usr/bin/env python3
"""Create a portable source bundle for external release evidence collection."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import subprocess
import stat
import sys
import tarfile
import tempfile
from pathlib import Path
from typing import Any

sys.dont_write_bytecode = True

from release_gate_audit import DEFAULT_DESKTOP_REPO
from release_gate_audit import DESKTOP_SOURCE_IDENTITY_PATHS
from release_gate_audit import RUNTIME_SOURCE_IDENTITY_PATHS
from release_gate_audit import compute_review_scope_identity
from release_gate_audit import compute_source_identity
from release_gate_audit import source_file_paths


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_OUTPUT_DIR = ROOT / "target" / "release-evidence-source-bundle"
RUNTIME_BUNDLE_DIR = "agent-workspace-linux"
DESKTOP_BUNDLE_DIR = "codex-desktop-linux"
VIEWER_COLLECT_SCRIPT = "collect-viewer-evidence.sh"
APP_QA_COLLECT_SCRIPT = "collect-app-qa-evidence.sh"
GITHUB_EXPLORE_COLLECT_SCRIPT = "collect-github-explore-evidence.sh"
HUMAN_REVIEW_SCRIPT = "create-human-review-marker.sh"
README_FILE = "README-collect-release-evidence.md"
MANIFEST_FILE = "release-evidence-source-bundle.json"


def normalized_tarinfo(tarinfo: tarfile.TarInfo) -> tarfile.TarInfo:
    tarinfo.uid = 0
    tarinfo.gid = 0
    tarinfo.uname = ""
    tarinfo.gname = ""
    tarinfo.mtime = 0
    return tarinfo


def write_text(path: Path, content: str, *, executable: bool = False) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    if executable:
        path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def copy_file(src_root: Path, dest_root: Path, rel_path: str) -> None:
    src = src_root / rel_path
    dest = dest_root / rel_path
    if not src.is_file():
        return
    dest.parent.mkdir(parents=True, exist_ok=True)
    data = src.read_bytes()
    dest.write_bytes(data)
    dest.chmod(src.stat().st_mode & 0o777)


def copy_source_paths(src_root: Path, dest_root: Path, source_paths: list[str]) -> list[str]:
    copied: list[str] = []
    for rel_path in source_file_paths(src_root, source_paths):
        copy_file(src_root, dest_root, rel_path)
        if (dest_root / rel_path).is_file():
            copied.append(rel_path)
    return copied


def collection_readme(manifest: dict[str, Any]) -> str:
    source_hash = manifest["source_identity"]["source_hash"]
    return f"""# Agent Workspace Linux Release Evidence Bundle

This bundle contains the runtime source and sibling Codex Desktop feature source
needed to collect external viewer release evidence for source hash:

`{source_hash}`

## Collect A Viewer Row

From the extracted bundle root:

```bash
./{VIEWER_COLLECT_SCRIPT}
```

Use the extracted runtime binary and collector scripts as the evidence source.
Do not substitute Codex app MCP, Computer Use MCP, Playwright MCP, or Codex
Desktop bridge behavior for these rows; the release audit expects repo-owned
runtime evidence.
The collector reports include an `evidence_boundary` object, and the importer
rejects viewer/app-QA/GitHub Explore reports that do not prove repo-owned runtime
collection. Bundled collectors also verify the extracted runtime and Desktop
source bytes against the bundle manifest before stamping that manifest's source
identity into evidence reports, and they do that before launching viewer,
workspace, or browser work.

For native Wayland compositor evidence, run from a real Wayland session after
observing positive compositor layer-shell/top-layer behavior:

```bash
NATIVE_WAYLAND_LAYER_SHELL_OBSERVED=1 \\
NATIVE_WAYLAND_LAYER_SHELL_NOTES="<compositor, desktop, observed behavior>" \\
./{VIEWER_COLLECT_SCRIPT}
```

The importer and release audit reject X11 sessions, missing notes,
GNOME/Xwayland fallback notes, and notes that say the viewer was not
layer-shell. Use this only for a positive native Wayland layer-shell/top-layer
observation.

Viewer rows also include display-server attestation with `lsof` process
evidence for the active host display socket. The importer and release audit
reject remote X forwarding and known nested/headless host display servers such
as Xvfb, xpra, Xephyr, and headless Weston; collect rows from the real desktop
session you want to prove.

The report is written under:

```text
{RUNTIME_BUNDLE_DIR}/target/viewer-desktop-matrix/
```

Copy that JSON report back to the release machine and import it with:

```bash
scripts/import_release_evidence.py /path/to/copied/report.json
scripts/release_gate_audit.py
```

## Create A Human Review Marker

After a human has reviewed the extracted runtime source, sibling Desktop feature
source, and the generated runtime/Desktop review artifacts, create a marker:

```bash
HUMAN_REVIEW_NOTES="<specific scope and acceptance notes>" \\
./{HUMAN_REVIEW_SCRIPT} --reviewer "$USER" --confirm-reviewed --notes "$HUMAN_REVIEW_NOTES"
```

The marker helper refuses to run if the extracted runtime or Desktop source
bytes no longer match the bundle manifest. Because extracted bundles do not
carry `.git` metadata, generated review artifacts include manifest-scoped
source inventories and patch-form views of the copied files.

The marker and review artifacts are written under:

```text
{RUNTIME_BUNDLE_DIR}/target/
```

Copy the marker JSON and the generated runtime/Desktop review artifact files
back to the release machine together, then import them with:

```bash
scripts/import_release_evidence.py /path/to/copied/human-review-marker-or-directory
```

The helper stamps the marker with this bundle's manifest source and review-scope
identity only after verifying the copied source bytes. The importer rejects
missing artifact bytes, source mismatches, and review-scope mismatches.

## Collect GitHub Explore Evidence

Run the bundled collector from the extracted bundle root:

```bash
./{GITHUB_EXPLORE_COLLECT_SCRIPT}
```

By default this opens the host-visible GPUI viewer immediately through
`workspace_open_viewer`, keeps it always on top, opens GitHub Explore in the
workspace Chrome/Chromium app, reads the page through workspace-owned Chrome
DevTools, and writes three repository recommendations. Use
`GITHUB_EXPLORE_OPEN_VIEWER=0` only when explicitly collecting in no-viewer
automation.

The report must prove `workspace_open_viewer` launch metadata,
`workspace_browser_targets`, `workspace_browser_snapshot`, a loopback DevTools
endpoint, screenshot/event evidence, and a clean workspace stop. Host Chrome
bridge, Codex app MCP, Computer Use MCP, curl, and Playwright evidence are
rejected.
The report is written under:

```text
{RUNTIME_BUNDLE_DIR}/target/github-explore-dogfood/
```

Copy the generated GitHub Explore JSON report back to the release machine and
import it with the same importer command above.

## Collect Local App-QA Evidence

If the local app-QA gate is missing for this source hash, run:

```bash
./{APP_QA_COLLECT_SCRIPT}
```

The report is written under:

```text
{RUNTIME_BUNDLE_DIR}/target/app-qa-dogfood/
```

Copy that JSON report back to the release machine and import it with the same
importer command above.

Do not edit the extracted source before collecting evidence. The release audit
rejects reports whose combined source identity does not match the release tree.
"""


def viewer_collection_script() -> str:
    return f"""#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${{BASH_SOURCE[0]}}")" && pwd)"
cd "$ROOT_DIR/{RUNTIME_BUNDLE_DIR}"

export CODEX_DESKTOP_LINUX_REPO="$ROOT_DIR/{DESKTOP_BUNDLE_DIR}"
export AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST="$ROOT_DIR/{MANIFEST_FILE}"
export REQUIRE_VIEWER_SMOKE="${{REQUIRE_VIEWER_SMOKE:-1}}"

exec scripts/viewer_desktop_matrix_probe.sh
"""


def github_explore_collection_script() -> str:
    return f"""#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${{BASH_SOURCE[0]}}")" && pwd)"
cd "$ROOT_DIR/{RUNTIME_BUNDLE_DIR}"

export CODEX_DESKTOP_LINUX_REPO="$ROOT_DIR/{DESKTOP_BUNDLE_DIR}"
export AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST="$ROOT_DIR/{MANIFEST_FILE}"

exec scripts/github_explore_dogfood_probe.js "$@"
"""


def app_qa_collection_script() -> str:
    return f"""#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${{BASH_SOURCE[0]}}")" && pwd)"
cd "$ROOT_DIR/{RUNTIME_BUNDLE_DIR}"

export CODEX_DESKTOP_LINUX_REPO="$ROOT_DIR/{DESKTOP_BUNDLE_DIR}"
export AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST="$ROOT_DIR/{MANIFEST_FILE}"

exec scripts/app_qa_dogfood_smoke.sh
"""


def human_review_marker_script() -> str:
    return f"""#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${{BASH_SOURCE[0]}}")" && pwd)"
cd "$ROOT_DIR/{RUNTIME_BUNDLE_DIR}"

export CODEX_DESKTOP_LINUX_REPO="$ROOT_DIR/{DESKTOP_BUNDLE_DIR}"
export AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST="$ROOT_DIR/{MANIFEST_FILE}"

exec scripts/create_human_review_marker.py "$@"
"""


def build_bundle(
    *,
    output_dir: Path,
    desktop_repo: Path,
    force_name: str | None = None,
) -> tuple[Path, dict[str, Any]]:
    source_identity = compute_source_identity(ROOT, desktop_repo=desktop_repo)
    review_scope_identity = compute_review_scope_identity(ROOT, desktop_repo=desktop_repo)
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    bundle_name = force_name or f"{stamp}-{source_identity['source_hash'][:12]}"
    tar_path = output_dir / f"{bundle_name}.tar.gz"

    with tempfile.TemporaryDirectory(prefix="agent-workspace-release-bundle-") as temp:
        staging = Path(temp) / bundle_name
        runtime_dest = staging / RUNTIME_BUNDLE_DIR
        desktop_dest = staging / DESKTOP_BUNDLE_DIR

        runtime_files = copy_source_paths(ROOT, runtime_dest, RUNTIME_SOURCE_IDENTITY_PATHS)
        desktop_files = copy_source_paths(
            desktop_repo,
            desktop_dest,
            DESKTOP_SOURCE_IDENTITY_PATHS,
        )

        manifest: dict[str, Any] = {
            "schema": "agent-workspace-linux.release_evidence_source_bundle.v1",
            "created_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
            "source_identity": source_identity,
            "review_scope_identity": review_scope_identity,
            "runtime_dir": RUNTIME_BUNDLE_DIR,
            "desktop_dir": DESKTOP_BUNDLE_DIR,
            "runtime_files": runtime_files,
            "desktop_files": desktop_files,
            "commands": {
                "collect_viewer_row": f"./{VIEWER_COLLECT_SCRIPT}",
                "collect_app_qa": f"./{APP_QA_COLLECT_SCRIPT}",
                "collect_native_wayland_row": (
                    "NATIVE_WAYLAND_LAYER_SHELL_OBSERVED=1 "
                    "NATIVE_WAYLAND_LAYER_SHELL_NOTES='<compositor, desktop, observed positive layer-shell/top-layer behavior>' "
                    f"./{VIEWER_COLLECT_SCRIPT}"
                ),
                "collect_github_explore": f"./{GITHUB_EXPLORE_COLLECT_SCRIPT}",
                "create_human_review_marker": (
                    f"./{HUMAN_REVIEW_SCRIPT} "
                    "--reviewer \"$USER\" "
                    "--confirm-reviewed "
                    "--notes \"$HUMAN_REVIEW_NOTES\""
                ),
                "import_report": "scripts/import_release_evidence.py /path/to/copied/report.json",
            },
        }
        write_text(staging / MANIFEST_FILE, json.dumps(manifest, indent=2, sort_keys=True) + "\n")
        write_text(staging / README_FILE, collection_readme(manifest))
        write_text(staging / VIEWER_COLLECT_SCRIPT, viewer_collection_script(), executable=True)
        write_text(staging / APP_QA_COLLECT_SCRIPT, app_qa_collection_script(), executable=True)
        write_text(
            staging / GITHUB_EXPLORE_COLLECT_SCRIPT,
            github_explore_collection_script(),
            executable=True,
        )
        write_text(staging / HUMAN_REVIEW_SCRIPT, human_review_marker_script(), executable=True)

        output_dir.mkdir(parents=True, exist_ok=True)
        if tar_path.exists():
            raise FileExistsError(f"bundle already exists: {tar_path}")
        with tarfile.open(tar_path, "w:gz") as archive:
            archive.add(staging, arcname=bundle_name, filter=normalized_tarinfo)

    return tar_path, manifest


def inspect_bundle(path: Path) -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix="agent-workspace-release-bundle-check-") as temp:
        temp_root = Path(temp)
        with tarfile.open(path, "r:gz") as archive:
            archive.extractall(temp_root)
        roots = [item for item in temp_root.iterdir() if item.is_dir()]
        if len(roots) != 1:
            raise AssertionError("bundle should contain exactly one root directory")
        root = roots[0]
        manifest = json.loads((root / MANIFEST_FILE).read_text(encoding="utf-8"))
        expected_paths = [
            root / RUNTIME_BUNDLE_DIR / "Cargo.toml",
            root / RUNTIME_BUNDLE_DIR / "Cargo.lock",
            root / RUNTIME_BUNDLE_DIR / "src",
            root / RUNTIME_BUNDLE_DIR / "scripts" / "viewer_desktop_matrix_probe.sh",
            root / RUNTIME_BUNDLE_DIR / "scripts" / "app_qa_dogfood_smoke.sh",
            root / RUNTIME_BUNDLE_DIR / "scripts" / "github_explore_dogfood_probe.js",
            root / DESKTOP_BUNDLE_DIR / "linux-features" / "agent-workspace",
            root / VIEWER_COLLECT_SCRIPT,
            root / APP_QA_COLLECT_SCRIPT,
            root / GITHUB_EXPLORE_COLLECT_SCRIPT,
            root / HUMAN_REVIEW_SCRIPT,
            root / README_FILE,
        ]
        missing = [str(item.relative_to(root)) for item in expected_paths if not item.exists()]
        if missing:
            raise AssertionError(f"bundle missing expected paths: {missing}")
        for script_name in [
            VIEWER_COLLECT_SCRIPT,
            APP_QA_COLLECT_SCRIPT,
            GITHUB_EXPLORE_COLLECT_SCRIPT,
            HUMAN_REVIEW_SCRIPT,
        ]:
            script_path = root / script_name
            if not os.access(script_path, os.X_OK):
                raise AssertionError(f"bundle collector is not executable: {script_name}")
        viewer_dry_run_dir = root / RUNTIME_BUNDLE_DIR / "target" / "viewer-bundle-self-test"
        viewer_dry_run = subprocess.run(
            [str(root / VIEWER_COLLECT_SCRIPT)],
            cwd=root,
            env={
                **os.environ,
                "RUN_VIEWER_SMOKE": "0",
                "REQUIRE_VIEWER_SMOKE": "0",
                "OUTPUT_DIR": str(viewer_dry_run_dir),
            },
            check=False,
            capture_output=True,
            text=True,
            timeout=60,
        )
        if viewer_dry_run.returncode != 0:
            raise AssertionError(
                "extracted viewer collector dry-run failed:\n"
                f"stdout:\n{viewer_dry_run.stdout}\n"
                f"stderr:\n{viewer_dry_run.stderr}"
            )
        viewer_reports = sorted(viewer_dry_run_dir.glob("*.json"))
        if len(viewer_reports) != 1:
            raise AssertionError(
                f"extracted viewer collector dry-run wrote {len(viewer_reports)} reports"
            )
        viewer_report = json.loads(viewer_reports[0].read_text(encoding="utf-8"))
        if viewer_report.get("schema") != "agent-workspace-linux.viewer_desktop_matrix.v1":
            raise AssertionError("viewer collector dry-run report has the wrong schema")
        boundary = viewer_report.get("evidence_boundary") or {}
        if (
            boundary.get("collector") != "agent-workspace-linux"
            or boundary.get("repo_owned_runtime") is not True
            or boundary.get("codex_app_mcp_used") is not False
            or boundary.get("computer_use_mcp_used") is not False
            or boundary.get("codex_desktop_bridge_used") is not False
            or boundary.get("playwright_mcp_used") is not False
        ):
            raise AssertionError("viewer collector dry-run report lacks repo-owned evidence boundary")
        if (viewer_report.get("viewer_smoke") or {}).get("status") != "skipped":
            raise AssertionError("viewer collector dry-run must not count as release smoke")
        if (
            (viewer_report.get("source_identity") or {}).get("source_hash")
            != (manifest.get("source_identity") or {}).get("source_hash")
        ):
            raise AssertionError("viewer collector dry-run source identity does not match bundle manifest")
        tamper_path = root / RUNTIME_BUNDLE_DIR / "src" / "main.rs"
        original_tamper_bytes = tamper_path.read_bytes()
        tampered_viewer_dir = root / RUNTIME_BUNDLE_DIR / "target" / "viewer-bundle-tamper-test"
        try:
            tamper_path.write_bytes(original_tamper_bytes + b"\n// bundle self-test tamper\n")
            tampered_viewer = subprocess.run(
                [str(root / VIEWER_COLLECT_SCRIPT)],
                cwd=root,
                env={
                    **os.environ,
                    "RUN_VIEWER_SMOKE": "0",
                    "REQUIRE_VIEWER_SMOKE": "0",
                    "OUTPUT_DIR": str(tampered_viewer_dir),
                },
                check=False,
                capture_output=True,
                text=True,
                timeout=60,
            )
            if tampered_viewer.returncode == 0:
                raise AssertionError(
                    "tampered extracted viewer collector unexpectedly accepted changed source bytes"
                )
            if "source bytes no longer match" not in (
                f"{tampered_viewer.stdout}\n{tampered_viewer.stderr}"
            ):
                raise AssertionError(
                    "tampered extracted viewer collector failed without the source-byte guard message:\n"
                    f"stdout:\n{tampered_viewer.stdout}\n"
                    f"stderr:\n{tampered_viewer.stderr}"
                )
            if list(tampered_viewer_dir.glob("*.json")):
                raise AssertionError(
                    "tampered extracted viewer collector must not write release evidence"
                )
            tampered_github = subprocess.run(
                [str(root / GITHUB_EXPLORE_COLLECT_SCRIPT), "--self-test"],
                cwd=root,
                check=False,
                capture_output=True,
                text=True,
                timeout=60,
            )
            if tampered_github.returncode == 0:
                raise AssertionError(
                    "tampered extracted GitHub Explore collector unexpectedly accepted changed source bytes"
                )
            if "source bytes no longer match" not in (
                f"{tampered_github.stdout}\n{tampered_github.stderr}"
            ):
                raise AssertionError(
                    "tampered extracted GitHub Explore collector failed without the source-byte guard message:\n"
                    f"stdout:\n{tampered_github.stdout}\n"
                    f"stderr:\n{tampered_github.stderr}"
                )
            tampered_app_qa_dir = root / RUNTIME_BUNDLE_DIR / "target" / "app-qa-bundle-tamper-test"
            tampered_app_qa = subprocess.run(
                [str(root / APP_QA_COLLECT_SCRIPT)],
                cwd=root,
                env={
                    **os.environ,
                    "OUTPUT_DIR": str(tampered_app_qa_dir),
                },
                check=False,
                capture_output=True,
                text=True,
                timeout=60,
            )
            if tampered_app_qa.returncode == 0:
                raise AssertionError(
                    "tampered extracted app-QA collector unexpectedly accepted changed source bytes"
                )
            if "source bytes no longer match" not in (
                f"{tampered_app_qa.stdout}\n{tampered_app_qa.stderr}"
            ):
                raise AssertionError(
                    "tampered extracted app-QA collector failed without the source-byte guard message:\n"
                    f"stdout:\n{tampered_app_qa.stdout}\n"
                    f"stderr:\n{tampered_app_qa.stderr}"
                )
            if list(tampered_app_qa_dir.glob("*.json")):
                raise AssertionError(
                    "tampered extracted app-QA collector must not write release evidence"
                )
        finally:
            tamper_path.write_bytes(original_tamper_bytes)
        github_self_test = subprocess.run(
            [str(root / GITHUB_EXPLORE_COLLECT_SCRIPT), "--self-test"],
            cwd=root,
            check=False,
            capture_output=True,
            text=True,
            timeout=60,
        )
        if github_self_test.returncode != 0:
            raise AssertionError(
                "extracted GitHub Explore collector self-test failed:\n"
                f"stdout:\n{github_self_test.stdout}\n"
                f"stderr:\n{github_self_test.stderr}"
            )
        human_marker_self_test = subprocess.run(
            [str(root / HUMAN_REVIEW_SCRIPT), "--self-test"],
            cwd=root,
            check=False,
            capture_output=True,
            text=True,
            timeout=60,
        )
        if human_marker_self_test.returncode != 0:
            raise AssertionError(
                "extracted human review marker self-test failed:\n"
                f"stdout:\n{human_marker_self_test.stdout}\n"
                f"stderr:\n{human_marker_self_test.stderr}"
            )
        inspected = dict(manifest)
        inspected["readme_text"] = (root / README_FILE).read_text(encoding="utf-8")
        return inspected


def run_self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="agent-workspace-release-bundle-self-test-") as temp:
        output_dir = Path(temp)
        tar_path, manifest = build_bundle(
            output_dir=output_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            force_name="self-test-bundle",
        )
        inspected = inspect_bundle(tar_path)
        assert inspected["schema"] == "agent-workspace-linux.release_evidence_source_bundle.v1"
        assert inspected["source_identity"]["source_hash"] == manifest["source_identity"]["source_hash"]
        assert inspected["commands"]["collect_viewer_row"] == f"./{VIEWER_COLLECT_SCRIPT}"
        assert inspected["commands"]["collect_app_qa"] == f"./{APP_QA_COLLECT_SCRIPT}"
        assert inspected["commands"]["collect_github_explore"] == f"./{GITHUB_EXPLORE_COLLECT_SCRIPT}"
        assert "workspace_open_viewer" in inspected["readme_text"]
        assert "workspace_browser_targets" in inspected["readme_text"]
        assert "workspace_browser_snapshot" in inspected["readme_text"]
        assert "loopback DevTools" in inspected["readme_text"]
        assert "endpoint" in inspected["readme_text"]
        assert "before launching viewer" in inspected["readme_text"]
        assert "refuses to run if the extracted runtime or Desktop source" in inspected["readme_text"]
        assert "source inventories and patch-form views" in inspected["readme_text"]
        assert inspected["commands"]["create_human_review_marker"].startswith(
            f"./{HUMAN_REVIEW_SCRIPT}"
        )
    print("release evidence source bundle self-test passed")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT_DIR)
    parser.add_argument(
        "--desktop-repo",
        type=Path,
        default=DEFAULT_DESKTOP_REPO,
        help="sibling Codex Desktop repo to include for combined source identity",
    )
    parser.add_argument("--json", action="store_true", help="print machine-readable result")
    parser.add_argument("--self-test", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.self_test:
        run_self_test()
        return 0

    tar_path, manifest = build_bundle(output_dir=args.output_dir, desktop_repo=args.desktop_repo)
    result = {
        "schema": "agent-workspace-linux.release_evidence_source_bundle_export.v1",
        "bundle_path": str(tar_path),
        "source_identity": manifest["source_identity"],
        "review_scope_identity": manifest["review_scope_identity"],
        "commands": manifest["commands"],
    }
    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print(f"release evidence source bundle: {tar_path}")
        print(f"source hash: {manifest['source_identity']['source_hash']}")
        print(f"review scope hash: {manifest['review_scope_identity']['review_scope_hash']}")
        print(f"collect viewer row after extracting: ./{VIEWER_COLLECT_SCRIPT}")
        print(f"collect app-QA evidence after extracting: ./{APP_QA_COLLECT_SCRIPT}")
        print(f"collect GitHub Explore evidence after extracting: ./{GITHUB_EXPLORE_COLLECT_SCRIPT}")
        print(f"create human review marker after extracting: ./{HUMAN_REVIEW_SCRIPT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
