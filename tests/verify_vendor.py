#!/usr/bin/env python3
"""Offline provenance check: unchanged registry sources plus one exact reverse patch."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    manifest = json.loads((ROOT/'vendor/eframe-upstream.json').read_text())
    package = ROOT/'vendor'/f"{manifest['name']}-{manifest['version']}"
    expected = dict(manifest['files'], **manifest['license_files'])
    actual = {str(p.relative_to(package)) for p in package.rglob('*') if p.is_file()}
    assert actual == set(expected), 'Unexpected/missing third-party source files'
    for name, digest in expected.items():
        path = package/name
        assert not path.is_symlink(), 'Vendored files must be regular files'
        if name == manifest['patched_file']:digest = manifest['patched_sha256']
        assert hashlib.sha256(path.read_bytes()).hexdigest() == digest, f'Unexpected vendor change: {name}'
    patch = ROOT/'vendor/eframe-window-retirement.patch'
    assert hashlib.sha256(patch.read_bytes()).hexdigest() == manifest['patch_sha256']
    with tempfile.TemporaryDirectory(prefix='sidecar-vendor-check-') as temporary:
        restored = Path(temporary)/'run.rs'
        shutil.copyfile(package/manifest['patched_file'],restored)
        subprocess.run(['patch','--batch','--fuzz=0','--reverse',str(restored),str(patch)],
            check=True,capture_output=True,text=True,timeout=5)
        assert hashlib.sha256(restored.read_bytes()).hexdigest() == manifest['files'][manifest['patched_file']], 'Patch does not reconstruct the reviewed upstream source'
    print('PASS: pinned eframe sources/licenses and exact, reversible one-file retirement patch')


if __name__ == '__main__':main()
