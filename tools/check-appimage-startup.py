#!/usr/bin/env python3
"""Start the real AppImage on an isolated display, D-Bus session and profile."""

import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time


def descendants(parent):
    processes = {}
    for proc in Path('/proc').iterdir():
        if not proc.name.isdecimal():
            continue
        try:
            # The process name in parentheses can contain spaces.
            fields = (proc / 'stat').read_text().rsplit(')', 1)[1].split()
            processes[int(proc.name)] = int(fields[1])
        except (OSError, ValueError, IndexError):
            continue
    found = {parent}
    while True:
        children = {pid for pid, ppid in processes.items() if ppid in found}
        if children <= found:
            return found
        found.update(children)


def check_libraries(proc, appdir):
    libraries = []
    for line in (proc / 'maps').read_text().splitlines():
        fields = line.split(None, 5)
        if len(fields) == 6 and fields[5].startswith('/'):
            libraries.append(Path(fields[5]).resolve())
    for name in ('libglib-2.0.so', 'libgio-2.0.so'):
        matches = [p for p in libraries if p.name.startswith(name)]
        if not matches or any(not p.is_relative_to(appdir) for p in matches):
            raise RuntimeError(f'{name} does not come from the AppImage (PID {proc.name})')
    if any('/gio/modules/' in str(p) and not p.is_relative_to(appdir)
           for p in libraries):
        raise RuntimeError(f'A host GIO module was loaded (PID {proc.name})')


def check_processes(parent):
    runtime = None
    webviews = []
    runtime_appdir = None
    for pid in descendants(parent):
        proc = Path('/proc') / str(pid)
        try:
            executable = (proc / 'exe').resolve(strict=True)
            if executable.name in ('WebKitWebProcess', 'WebKitNetworkProcess'):
                webviews.append((proc, executable))
            if executable.name != 'rustory':
                continue
            environment = dict(
                entry.split(b'=', 1)
                for entry in (proc / 'environ').read_bytes().split(b'\0')
                if b'=' in entry
            )
            appdir = Path(os.fsdecode(environment[b'APPDIR'])).resolve(strict=True)
            if executable != appdir / 'usr/bin/rustory':
                continue
            check_libraries(proc, appdir)
            runtime = pid
            runtime_appdir = appdir
        except (FileNotFoundError, ProcessLookupError, KeyError):
            continue
    if runtime is None:
        raise RuntimeError('The real Rustory AppImage process is not running')
    if not any(path.name == 'WebKitWebProcess' for _, path in webviews):
        raise RuntimeError('The bundled WebKit webview process did not start')
    for proc, path in webviews:
        if not path.is_relative_to(runtime_appdir):
            raise RuntimeError(f'WebKit executable is outside the AppImage: {path}')
        check_libraries(proc, runtime_appdir)
    return runtime


def main():
    if len(sys.argv) != 2:
        raise SystemExit(f'Usage: {sys.argv[0]} path/to/Rustory.AppImage')
    appimage = Path(sys.argv[1]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix='rustory-appimage-startup-') as directory:
        root = Path(directory)
        environment = os.environ.copy()
        for key in list(environment):
            if key.startswith(('GIO_', 'GST_', 'GI_', 'GTK_', 'GDK_', 'WEBKIT_')) or key in (
                'LD_LIBRARY_PATH', 'LD_PRELOAD', 'APPDIR', 'APPIMAGE',
                'DBUS_SESSION_BUS_ADDRESS', 'DBUS_SESSION_BUS_PID',
            ):
                environment.pop(key)
        for variable, child in (
            ('HOME', 'home'), ('XDG_DATA_HOME', 'data'),
            ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
            ('XDG_RUNTIME_DIR', 'runtime'),
        ):
            path = root / child
            path.mkdir(mode=0o700)
            environment[variable] = str(path)
        environment['GSETTINGS_BACKEND'] = 'memory'
        environment['RUSTORY_DEVICE_AUTOMOUNT'] = '0'
        # Do not activate host portal/GVFS services or mount a document portal
        # from a smoke test. Rustory still gets its own functioning session bus.
        bus_config = root / 'session.conf'
        bus_config.write_text('''<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>\n''')
        log_path = root / 'startup.log'
        with log_path.open('wb') as log:
            process = subprocess.Popen(
                ['dbus-run-session', '--config-file', str(bus_config), '--',
                 'xvfb-run', '-a', str(appimage)],
                env=environment, stdout=log, stderr=subprocess.STDOUT,
                start_new_session=True,
            )
        try:
            try:
                process.wait(timeout=20)
                raise RuntimeError(f'AppImage exited during startup ({process.returncode})')
            except subprocess.TimeoutExpired:
                pass
            contents = log_path.read_text(errors='replace')
            if re.search(
                r'undefined symbol|Failed to load module|GStreamer element .+ not found'
                r'|error while loading shared libraries|Rustory AppImage:',
                contents,
            ):
                raise RuntimeError('The AppImage reported a runtime loading error')
            pid = check_processes(process.pid)
            print(f'AppImage startup passed: Rustory PID {pid}, WebKit started, bundled GLib/GIO.')
        except Exception:
            print(log_path.read_text(errors='replace'), file=sys.stderr)
            raise
        finally:
            # Only the process group created for this isolated test is stopped.
            try:
                os.killpg(process.pid, signal.SIGTERM)
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline:
                    process.poll()
                    os.killpg(process.pid, 0)
                    time.sleep(0.1)
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait(timeout=5)


if __name__ == '__main__':
    main()
