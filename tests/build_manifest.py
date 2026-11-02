#!/usr/bin/env python3
"""Build-time artifact identity; never collects user configuration or telemetry."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
BIN=ROOT/'target/release'
def main():
    with tempfile.TemporaryDirectory(prefix='igpu-man-build-info-') as temporary:
        result=subprocess.run([str(BIN/'igpu-man'),'--version'],env=dict(os.environ,XDG_STATE_HOME=temporary),text=True,capture_output=True,check=True,timeout=5)
    lines=result.stdout.strip().splitlines()
    assert len(lines)>=4 and lines[1].startswith('source_sha256 '),'Build release binaries with current build metadata first'
    data={'schema_version':1,'version_output':lines,'artifacts':{}}
    for name in ['igpu-man','igpu-mand','igpu-man-desktop']:
        with (BIN/name).open('rb') as binary:digest=hashlib.file_digest(binary,'sha256').hexdigest()
        with tempfile.TemporaryDirectory(prefix='igpu-man-build-version-') as state:
            version=subprocess.run([str(BIN/name),'--version'],env=dict(os.environ,XDG_STATE_HOME=state),text=True,capture_output=True,check=True,timeout=5).stdout.strip().splitlines()
        assert version[1:]==lines[1:],'Workspace binary versions/source IDs differ; rebuild the entire workspace'
        data['artifacts'][name]={'sha256':digest,'bytes':(BIN/name).stat().st_size,'version_output':version}
    (BIN/'build-info.json').write_text(json.dumps(data,indent=2)+'\n')
if __name__=='__main__':main()
