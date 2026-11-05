"""Fail-closed bus admission for mutating integration fixtures.

Temporary XDG directories do not isolate D-Bus. Call require_empty() before
staging or starting processes, and require_collector(child) before mutations.
Live, read-only hardware checks must explicitly opt out at their entry point.
"""
import os
import subprocess

COLLECTOR = 'io.github.igpu_man.Collector'
DASHBOARD = 'io.github.igpu_man.Dashboard'


def bus_call(method, name):
    address = os.environ.get('DBUS_SESSION_BUS_ADDRESS')
    if not address:
        raise RuntimeError('Refusing implicit session bus; use dbus-run-session')
    return subprocess.check_output([
        'busctl', f'--address={address}', '--timeout=3', 'call',
        'org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus',
        method, 's', name,
    ], text=True, stderr=subprocess.DEVNULL, timeout=5).strip()


def require_unowned(*names):
    for name in names:
        if bus_call('NameHasOwner', name) != 'b false':
            raise RuntimeError('Refusing existing Sidecar/user-manager bus owners; use an isolated test bus')


def require_empty():
    require_unowned(COLLECTOR, DASHBOARD, 'org.freedesktop.systemd1')

def require_collector(child):
    """Only a live, explicitly spawned fixture child may own the collector name."""
    if child.poll() is not None or bus_call('GetConnectionUnixProcessID', COLLECTOR) != f'u {child.pid}':
        raise RuntimeError('Fixture collector does not own its expected bus name; refusing mutations')
