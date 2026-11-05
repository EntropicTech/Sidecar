#!/usr/bin/env python3
"""Deterministic source release + checksum-pinned Arch recipe/SRCINFO. No upload."""
import gzip,hashlib,io,os,shutil,subprocess,tarfile,tomllib
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
INCLUDE=['Cargo.toml','Cargo.lock','Makefile','LICENSE','README.md','AGENTS.md','CONTRIBUTING.md','.gitignore','.github','crates','vendor','data','docs','tests','packaging','CHANGELOG.md','SECURITY.md']
def main():
 version=tomllib.loads((ROOT/'Cargo.toml').read_text())['workspace']['package']['version']
 dist=ROOT/'dist';dist.mkdir(exist_ok=True)
 name=f'igpu-man-{version}.tar.gz';archive=dist/name;epoch=int(os.environ.get('SOURCE_DATE_EPOCH','0'))
 files=[]
 for name_ in INCLUDE:
  path=ROOT/name_
  if not path.exists():continue
  files.extend([path] if path.is_file() else [p for p in path.rglob('*') if p.is_file() and '__pycache__' not in p.parts])
 with archive.open('wb') as raw:
  with gzip.GzipFile(filename='',mode='wb',fileobj=raw,mtime=epoch) as compressed:
   with tarfile.open(fileobj=compressed,mode='w|',format=tarfile.PAX_FORMAT) as tar:
    for path in sorted(set(files)):
     if path.is_symlink():raise SystemExit('Source release cannot contain symlinks')
     data=path.read_bytes();info=tarfile.TarInfo(f'igpu-man-{version}/'+str(path.relative_to(ROOT)))
     info.size=len(data);info.mtime=epoch;info.uid=info.gid=0;info.uname=info.gname='';info.mode=0o755 if os.access(path,os.X_OK) else 0o644
     tar.addfile(info,io.BytesIO(data))
 digest=hashlib.sha256(archive.read_bytes()).hexdigest()
 template=(ROOT/'packaging/PKGBUILD').read_text()
 recipe=template.replace('@SOURCE_SHA256@',digest)
 if '@SOURCE_SHA256@' in recipe or "'SKIP'" in recipe:raise SystemExit('Unpinned source recipe')
 (dist/'PKGBUILD').write_text(recipe)
 (dist/'SHA256SUMS').write_text(f'{digest}  {archive.name}\n')
 if subprocess.run(['bash','-n',str(dist/'PKGBUILD')]).returncode:raise SystemExit('Invalid PKGBUILD')
 if shutil.which('makepkg'):
  p=subprocess.run(['makepkg','--printsrcinfo'],cwd=dist,text=True,capture_output=True)
  if p.returncode:raise SystemExit('makepkg --printsrcinfo failed')
  (dist/'.SRCINFO').write_text(p.stdout)
 else:
  (dist/'.SRCINFO').unlink(missing_ok=True)
  print('Arch makepkg unavailable: source assets generated; .SRCINFO/AUR submission requires Arch packaging tools')
 print(f'Prepared {archive.name}, SHA256SUMS and pinned PKGBUILD (not published)')
if __name__=='__main__':main()
