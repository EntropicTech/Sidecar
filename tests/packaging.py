#!/usr/bin/env python3
"""Validate release install/removal inside a temporary DESTDIR, never /usr."""
import os
import re
from pathlib import Path
import subprocess
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def run(*args, **options):
    result = subprocess.run(args, cwd=ROOT, text=True, capture_output=True, timeout=60, **options)
    assert result.returncode == 0, (args, result.stdout, result.stderr)
    return result.stdout


def check_unit_policy(text):
    # Protect the reviewed security/observability contract in rendered installs.
    import configparser
    unit = configparser.ConfigParser(interpolation=None)
    unit.read_string(text)
    service = unit['Service']
    assert service.getboolean('NoNewPrivileges')
    assert service['UMask'] == '0077'
    for key in ['PrivateTmp', 'ProtectSystem', 'PrivateUsers']:
        assert not service.getboolean(key), f'{key} would break same-account procfs accounting'


def check_documentation_links(root):
    for path in root.rglob("*.md"):
        for target in re.findall(r"!?\[[^\]]*\]\(([^)]+)\)", path.read_text()):
            target = target.split("#", 1)[0].strip().strip("<>")
            if not target or re.match(r"[a-zA-Z][a-zA-Z0-9+.-]*:", target):
                continue
            assert (path.parent / target).exists(), f"Installed documentation link missing: {path.relative_to(root)} -> {target}"


def main():
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    with tempfile.TemporaryDirectory(prefix="igpu-man-package-") as temporary:
        temporary = Path(temporary)
        stage = temporary / "stage"
        runtime=temporary/"runtime";runtime.mkdir(mode=0o700)
        environment = dict(os.environ, XDG_STATE_HOME=str(temporary / "state"),XDG_CONFIG_HOME=str(temporary/"config"),XDG_DATA_HOME=str(temporary/"data"),XDG_RUNTIME_DIR=str(runtime))
        run("make", f"DESTDIR={stage}", "PREFIX=/usr", "install")
        docroot = stage / "usr/share/doc/igpu-man"
        required = ["README.md", "CHANGELOG.md", "LICENSE", "data/config.example.toml", "AGENTS.md", "CONTRIBUTING.md", "SECURITY.md", "config.example.toml", "build-info.json", "igpu-man-autostart.desktop", "vendor/README.md", "vendor/eframe-upstream.json", "vendor/eframe-window-retirement.patch", "vendor/LICENSE-MIT", "vendor/LICENSE-APACHE"]
        required += [str(path.relative_to(ROOT)) for path in (ROOT / "docs").rglob("*") if path.is_file()]
        assert all((docroot / path).is_file() for path in required)
        assert run(str(stage / "usr/bin/igpu-man"), "--version", env=environment).splitlines()[0] == f"igpu-man {version}"
        check_documentation_links(docroot)
        run("desktop-file-validate", str(stage / "usr/share/applications/igpu-man.desktop"))
        run("desktop-file-validate", str(docroot / "igpu-man-autostart.desktop"))
        check_unit_policy((stage / "usr/lib/systemd/user/igpu-man.service").read_text())
        unit = temporary / "igpu-man.service"
        unit.write_text((ROOT / "data/igpu-man.service").read_text().replace("ExecStart=/usr/bin/igpu-mand", f"ExecStart={stage}/usr/bin/igpu-mand"))
        run("systemd-analyze", "--user", "verify", str(unit),env=environment)
        run("bash", "-n", "packaging/PKGBUILD")
        sentinel = temporary / "user-data-retained"
        sentinel.write_text("private configuration and recordings are outside the install prefix")
        run("make", f"DESTDIR={stage}", "PREFIX=/usr", "uninstall")
        assert sentinel.exists() and not docroot.exists()
        assert not any(path.is_file() for path in stage.rglob("*"))
    # User installs have a different systemd search directory and must render
    # every executable path from PREFIX rather than retaining /usr or the checkout.
    with tempfile.TemporaryDirectory(prefix="sidecar-user-package-") as temporary:
        root=Path(temporary);stage=root/"stage";prefix=root/"home with spaces/.local"
        unitdir=prefix/"share/systemd/user"
        run("make",f"DESTDIR={stage}",f"PREFIX={prefix}",f"USER_UNIT_DIR={unitdir}","install")
        staged=lambda path:stage/path.relative_to("/")
        check_documentation_links(staged(prefix/"share/doc/igpu-man"))
        executable=prefix/"bin/igpu-man-desktop"
        desktop=staged(prefix/"share/applications/igpu-man.desktop")
        autostart=staged(prefix/"share/doc/igpu-man/igpu-man-autostart.desktop")
        unit=staged(unitdir/"igpu-man.service")
        assert f'Exec="{executable}"' in desktop.read_text()
        assert f'Exec="{executable}" --tray' in autostart.read_text()
        assert f'TryExec={executable}' in autostart.read_text()
        assert f'ExecStart="{prefix}/bin/igpu-mand"' in unit.read_text()
        check_unit_policy(unit.read_text())
        assert str(ROOT) not in desktop.read_text()+autostart.read_text()+unit.read_text()
        run("desktop-file-validate",str(desktop));run("desktop-file-validate",str(autostart))
        env=dict(os.environ,XDG_STATE_HOME=str(root/"state"))
        assert "source_sha256" in run(str(staged(prefix/"bin/igpu-man")),"--version",env=env)
        run("make",f"DESTDIR={stage}",f"PREFIX={prefix}",f"USER_UNIT_DIR={unitdir}","uninstall")
        assert not any(path.is_file() for path in stage.rglob("*"))
    run("make", "dist")
    with tarfile.open(ROOT / f"dist/igpu-man-{version}.tar.gz") as archive:
        names = set(archive.getnames())
    prefix = f"igpu-man-{version}/"
    for path in ["AGENTS.md", "CONTRIBUTING.md", ".github/workflows/check.yml", "tests/check_docs.py", "tests/packaging.py", "vendor/eframe-upstream.json", "vendor/eframe-0.31.1/src/native/run.rs", "vendor/eframe-window-retirement.patch"]:
        assert prefix + path in names, path
    for path in (ROOT / "docs").rglob("*"):
        if path.is_file():
            assert prefix + str(path.relative_to(ROOT)) in names, path
    print("PASS: system/user prefix-aware staged install/removal, every documentation asset, CLI version, desktop entry, user unit, PKGBUILD syntax, source archive")


if __name__ == "__main__":
    main()
