#!/usr/bin/env python3
"""Offline documentation contracts; semantic correctness still needs review."""
from pathlib import Path
import re
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def check():
    errors = []
    documents = [ROOT / p for p in ["README.md", "AGENTS.md", "CONTRIBUTING.md"]]
    documents += sorted((ROOT / "docs").rglob("*.md"))
    for path in documents:
        if not path.is_file():
            errors.append(f"Missing documentation: {path.relative_to(ROOT)}")
            continue
        text = path.read_text()
        for target in re.findall(r"!?\[[^\]]*\]\(([^)]+)\)", text):
            target = target.split("#", 1)[0].strip().strip("<>")
            if not target or re.match(r"[a-zA-Z][a-zA-Z0-9+.-]*:", target):
                continue
            if not (path.parent / target).exists():
                errors.append(f"Broken link in {path.relative_to(ROOT)}: {target}")
    readme = (ROOT / "README.md").read_text()
    index=(ROOT/"docs/README.md").read_text()
    indexed={ (ROOT/"docs"/target.split("#",1)[0]).resolve() for target in re.findall(r"\[[^\]]*\]\(([^)]+)\)",index) if ":" not in target }
    for path in (ROOT/"docs").rglob("*.md"):
        if path.name!="README.md" and path.resolve() not in indexed:
            errors.append(f"Documentation index omits {path.relative_to(ROOT)}")
    cli = (ROOT / "crates/cli/src/main.rs").read_text().split("enum Commands {", 1)[1].split("enum ProfileCommand", 1)[0]
    for variant in re.findall(r"^    ([A-Z][A-Za-z0-9]+)\s*(?:\{|,)", cli, re.M):
        if variant in {"TryProfile", "InternalRequest"}:  # Internal argv-preserving trial wrapper.
            continue
        command = re.sub(r"(?<!^)(?=[A-Z])", "-", variant).lower()
        if not re.search(r"\bigpu-man\s+" + re.escape(command) + r"(?:\s|`|$)", readme):
            errors.append(f"README missing public CLI command: igpu-man {command}")
    api = (ROOT / "docs/api.md").read_text()
    protocol=(ROOT/"crates/core/src/api.rs").read_text().split("operations! {",1)[1].split("}",1)[0]
    operations=set(re.findall(r'[A-Z][A-Za-z]+\s*=>\s*"([a-z_]+)"',protocol))
    documented=set(re.findall(r'^\|\s*`([a-z_]+)`\s*\|',api,re.M))
    if not operations:
        errors.append("No collector operations found; update the source coverage parser")
    if operations!=documented:
        errors.append(f"API table mismatch: missing={sorted(operations-documented)}, stale={sorted(documented-operations)}")
    lifecycle=(ROOT/"crates/desktop/src/lifecycle.rs").read_text().split("impl Service {",1)[1].split("pub struct Session",1)[0]
    for method in [name[:1].upper()+name[1:] for name in re.findall(r"fn ([a-z_]+)\(",lifecycle)]:
        if f"`{method}`" not in api:
            errors.append(f"API reference missing dashboard method: {method}")
    config_source = (ROOT / "crates/core/src/config.rs").read_text().split("pub struct Config {", 1)[1].split("impl Default", 1)[0]
    fields = set(re.findall(r"pub ([a-z_]+):", config_source))
    example = tomllib.loads((ROOT / "data/config.example.toml").read_text())
    if set(example) - fields:
        errors.append(f"Unknown config example fields: {set(example) - fields}")
    if fields - set(example) - {"aliases", "profiles"}:
        errors.append(f"Config example missing fields: {fields - set(example) - {'aliases', 'profiles'}}")
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())
    version = cargo["workspace"]["package"]["version"]
    makefile = (ROOT / "Makefile").read_text()
    pkgbuild = (ROOT / "packaging/PKGBUILD").read_text()
    if f"VERSION := {version}" not in makefile or f"pkgver={version}" not in pkgbuild:
        errors.append("Cargo/Makefile/PKGBUILD versions disagree")
    for required in ["AGENTS.md", "CONTRIBUTING.md", "find docs -type f"]:
        if required not in makefile:
            errors.append(f"Install/distribution rules omit documentation coverage: {required}")
    if "check_docs.py" not in makefile or "--all-features" not in makefile:
        errors.append("make check must enforce documentation and all-feature Clippy")
    if "Closing the window exits **both dashboard and tray**" in readme:
        errors.append("README still describes the removed unconditional exit-on-close behavior")
    if errors:
        raise AssertionError("\n".join(errors))
    print(f"PASS: {len(documents)} documents; local links, CLI/API coverage, config example, versions, packaging contracts")


if __name__ == "__main__":
    try:
        check()
    except (AssertionError, ValueError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
