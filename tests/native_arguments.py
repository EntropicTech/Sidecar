#!/usr/bin/env python3
"""Help/version/errors must work without a bus, display, config, or database."""
import os
from pathlib import Path
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
BIN=Path(os.environ.get('IGPU_MAN_BIN_DIR',ROOT/'target/debug'))
def main():
    with tempfile.TemporaryDirectory(prefix='igpu-man-arguments-') as temporary:
        root=Path(temporary)
        env=dict(os.environ,XDG_CONFIG_HOME=str(root/'config'),XDG_DATA_HOME=str(root/'data'),XDG_STATE_HOME=str(root/'state'),DBUS_SESSION_BUS_ADDRESS=f'unix:path={root}/absent-bus',DISPLAY='',WAYLAND_DISPLAY='')
        for name in ['igpu-man','igpu-mand','igpu-man-desktop']:
            for arguments,code in [(['--help'],0),(['--version'],0),(['--not-an-option'],2)]:
                result=subprocess.run([str(BIN/name),*arguments],env=env,text=True,capture_output=True,timeout=5)
                assert result.returncode==code,(name,arguments,result.stderr)
                if arguments==['--version']:assert 'source_sha256 ' in result.stdout
        result=subprocess.run([str(BIN/'igpu-man-desktop'),'--close','--quit'],env=env,text=True,capture_output=True,timeout=5)
        assert result.returncode==2
        result=subprocess.run([str(BIN/'igpu-man-desktop'),'--tray','--applications'],env=env,text=True,capture_output=True,timeout=5)
        assert result.returncode==2
        assert not (root/'data').exists() and not (root/'config').exists()
    print('PASS: native/CLI help, source version and invalid arguments need no bus/display/storage')
if __name__=='__main__':main()
