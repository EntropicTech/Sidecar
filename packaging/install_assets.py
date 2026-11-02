#!/usr/bin/env python3
"""Render prefix-aware desktop/service assets at installation time, never runtime."""
import argparse
import os
from pathlib import Path
import tempfile
ROOT = Path(__file__).resolve().parents[1]

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--prefix',required=True)
    parser.add_argument('--unit-dir',required=True)
    parser.add_argument('--destdir',default='')
    args=parser.parse_args()
    prefix=Path(args.prefix);unit_dir=Path(args.unit_dir)
    if not prefix.is_absolute() or not unit_dir.is_absolute():parser.error('prefix and unit-dir must be absolute')
    if any(c in str(prefix)+str(unit_dir) for c in '\n\r\0'):parser.error('invalid install path')
    binary=str(prefix/'bin/igpu-man-desktop')
    # Desktop Exec has a separate escaping layer from ordinary key values.
    quoted='"'+binary.replace('\\','\\\\\\\\').replace('"','\\"').replace('`','\\`').replace('$','\\$').replace('%','%%')+'"'
    collector=str(prefix/'bin/igpu-mand').replace('\\','\\\\').replace('"','\\"').replace('%','%%').replace('$','$$')
    unit=(ROOT/'data/igpu-man.service').read_text().replace('ExecStart=/usr/bin/igpu-mand',f'ExecStart="{collector}"')
    desktop=(ROOT/'data/igpu-man.desktop').read_text().replace('Exec=igpu-man-desktop',f'Exec={quoted}')
    autostart=(ROOT/'data/igpu-man-autostart.desktop').read_text().replace('Exec=igpu-man-desktop --tray',f'Exec={quoted} --tray').replace('TryExec=igpu-man-desktop',f'TryExec={binary}')
    assets={unit_dir/'igpu-man.service':unit,prefix/'share/applications/igpu-man.desktop':desktop,prefix/'share/doc/igpu-man/igpu-man-autostart.desktop':autostart}
    for path,text in assets.items():
        destination=Path(args.destdir)/path.relative_to('/') if args.destdir else path
        destination.parent.mkdir(parents=True,exist_ok=True)
        fd,name=tempfile.mkstemp(prefix='.sidecar-install-',dir=destination.parent)
        try:
            with os.fdopen(fd,'w') as output:
                os.fchmod(output.fileno(),0o644);output.write(text);output.flush();os.fsync(output.fileno())
            os.replace(name,destination)
        finally:
            Path(name).unlink(missing_ok=True)
if __name__=='__main__':main()
