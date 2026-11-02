#!/usr/bin/env python3
"""Regenerate checked-in Sidecar icons; optional developer tool (librsvg)."""
from pathlib import Path
import subprocess
ROOT = Path(__file__).resolve().parents[1]
for size in [24, 32, 48, 128]:
    subprocess.run(['rsvg-convert', '--width', str(size), '--height', str(size),
                    '--output', str(ROOT / f'data/igpu-man-{size}.png'),
                    str(ROOT / 'data/igpu-man.svg')], check=True)
