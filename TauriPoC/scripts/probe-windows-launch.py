"""Measure Windows launch flags and GUI-owned Job lifetime with harmless children."""

import ctypes as ct
from ctypes import wintypes as wt
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time


CREATE_SUSPENDED = 0x00000004
DETACHED_PROCESS_GROUP = 0x00000008 | 0x00000200
BREAKAWAY = 0x01000000
KILL_ON_JOB_CLOSE = 0x00002000
WAIT_OBJECT_0 = 0
WAIT_TIMEOUT = 258
LAUNCH_MODES = {
    "plain": 0,
    "detached_group": DETACHED_PROCESS_GROUP,
    "breakaway_detached_group": BREAKAWAY | DETACHED_PROCESS_GROUP,
}


class StartupInfo(ct.Structure):
    _fields_ = [
        ("cb", wt.DWORD),
        ("reserved", wt.LPWSTR),
        ("desktop", wt.LPWSTR),
        ("title", wt.LPWSTR),
        ("x", wt.DWORD),
        ("y", wt.DWORD),
        ("x_size", wt.DWORD),
        ("y_size", wt.DWORD),
        ("x_chars", wt.DWORD),
        ("y_chars", wt.DWORD),
        ("fill", wt.DWORD),
        ("flags", wt.DWORD),
        ("show", wt.WORD),
        ("reserved_size", wt.WORD),
        ("reserved_bytes", ct.c_void_p),
        ("stdin", wt.HANDLE),
        ("stdout", wt.HANDLE),
        ("stderr", wt.HANDLE),
    ]


class ProcessInfo(ct.Structure):
    _fields_ = [
        ("process", wt.HANDLE),
        ("thread", wt.HANDLE),
        ("pid", wt.DWORD),
        ("tid", wt.DWORD),
    ]


class BasicLimits(ct.Structure):
    _fields_ = [
        ("process_time", ct.c_longlong),
        ("job_time", ct.c_longlong),
        ("flags", wt.DWORD),
        ("min_working_set", ct.c_size_t),
        ("max_working_set", ct.c_size_t),
        ("active_process_limit", wt.DWORD),
        ("affinity", ct.c_size_t),
        ("priority", wt.DWORD),
        ("scheduling", wt.DWORD),
    ]


class IoCounters(ct.Structure):
    _fields_ = [(name, ct.c_ulonglong) for name in (
        "read_ops", "write_ops", "other_ops", "read_bytes", "write_bytes", "other_bytes"
    )]


class ExtendedLimits(ct.Structure):
    _fields_ = [
        ("basic", BasicLimits),
        ("io", IoCounters),
        ("process_memory", ct.c_size_t),
        ("job_memory", ct.c_size_t),
        ("peak_process_memory", ct.c_size_t),
        ("peak_job_memory", ct.c_size_t),
    ]


class Windows:
    def __init__(self):
        self.kernel = ct.WinDLL("kernel32", use_last_error=True)
        signatures = {
            "GetCurrentProcess": ([], wt.HANDLE),
            "CreateProcessW": ([wt.LPCWSTR, wt.LPWSTR, ct.c_void_p, ct.c_void_p,
                                wt.BOOL, wt.DWORD, ct.c_void_p, wt.LPCWSTR,
                                ct.POINTER(StartupInfo), ct.POINTER(ProcessInfo)], wt.BOOL),
            "CloseHandle": ([wt.HANDLE], wt.BOOL),
            "TerminateProcess": ([wt.HANDLE, wt.UINT], wt.BOOL),
            "WaitForSingleObject": ([wt.HANDLE, wt.DWORD], wt.DWORD),
            "ResumeThread": ([wt.HANDLE], wt.DWORD),
            "CreateJobObjectW": ([ct.c_void_p, wt.LPCWSTR], wt.HANDLE),
            "SetInformationJobObject": ([wt.HANDLE, ct.c_int, ct.c_void_p, wt.DWORD], wt.BOOL),
            "QueryInformationJobObject": ([wt.HANDLE, ct.c_int, ct.c_void_p, wt.DWORD,
                                           ct.POINTER(wt.DWORD)], wt.BOOL),
            "AssignProcessToJobObject": ([wt.HANDLE, wt.HANDLE], wt.BOOL),
            "IsProcessInJob": ([wt.HANDLE, wt.HANDLE, ct.POINTER(wt.BOOL)], wt.BOOL),
            "DuplicateHandle": ([wt.HANDLE, wt.HANDLE, wt.HANDLE, ct.POINTER(wt.HANDLE),
                                 wt.DWORD, wt.BOOL, wt.DWORD], wt.BOOL),
        }
        for name, (arguments, result) in signatures.items():
            function = getattr(self.kernel, name)
            function.argtypes = arguments
            function.restype = result

    @staticmethod
    def check(result):
        if not result:
            raise ct.WinError(ct.get_last_error())
        return result

    def in_job(self, process, job=None):
        result = wt.BOOL()
        self.check(self.kernel.IsProcessInJob(process, job, ct.byref(result)))
        return bool(result.value)

    def job_facts(self, job=None):
        limits = ExtendedLimits()
        if not self.kernel.QueryInformationJobObject(
            job, 9, ct.byref(limits), ct.sizeof(limits), None
        ):
            return {"query_error": ct.get_last_error()}
        flags = limits.basic.flags
        return {
            "limit_flags": f"0x{flags:08x}",
            "breakaway_ok": bool(flags & 0x00000800),
            "silent_breakaway_ok": bool(flags & 0x00001000),
            "kill_on_job_close": bool(flags & KILL_ON_JOB_CLOSE),
            "active_process_limit": limits.basic.active_process_limit,
        }

    def current_facts(self):
        return {
            "in_any_job": self.in_job(self.kernel.GetCurrentProcess()),
            "immediate_job_limits": self.job_facts(),
        }

    def launch(self, role, directory, name, flags):
        command = ct.create_unicode_buffer(subprocess.list2cmdline([
            sys.executable, str(Path(__file__).resolve()), role, str(directory), name
        ]))
        startup = StartupInfo()
        startup.cb = ct.sizeof(startup)
        process = ProcessInfo()
        self.check(self.kernel.CreateProcessW(
            sys.executable, command, None, None, False, flags, None, None,
            ct.byref(startup), ct.byref(process)
        ))
        return process

    def cleanup(self, handle):
        try:
            state = self.kernel.WaitForSingleObject(handle, 0)
            if state == WAIT_TIMEOUT:
                self.check(self.kernel.TerminateProcess(handle, 90))
            elif state != WAIT_OBJECT_0:
                raise ct.WinError(ct.get_last_error())
            if self.kernel.WaitForSingleObject(handle, 5000) != WAIT_OBJECT_0:
                raise RuntimeError("owned fixture process did not exit within five seconds")
        finally:
            self.kernel.CloseHandle(handle)

    def cleanup_all(self, handles):
        errors = []
        for handle in handles:
            try:
                self.cleanup(handle)
            except (OSError, RuntimeError) as error:
                errors.append(str(error))
        if errors:
            raise RuntimeError("; ".join(errors))


def write_json(path, value):
    pending = path.with_suffix(".pending")
    pending.write_text(json.dumps(value), encoding="utf-8")
    pending.replace(path)


def read_json(path, process, api):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if path.exists():
            return json.loads(path.read_text(encoding="utf-8"))
        if api.kernel.WaitForSingleObject(process, 0) != WAIT_TIMEOUT:
            raise RuntimeError(f"fixture exited before publishing {path.name}")
        time.sleep(0.02)
    raise RuntimeError(f"fixture did not publish {path.name} within ten seconds")


def child(api, directory, name):
    write_json(directory / f"{name}.json", api.current_facts())
    time.sleep(30)


def gui(api, directory):
    owned = []
    job = api.check(api.kernel.CreateJobObjectW(None, None))
    try:
        limits = ExtendedLimits()
        limits.basic.flags = KILL_ON_JOB_CLOSE
        api.check(api.kernel.SetInformationJobObject(
            job, 9, ct.byref(limits), ct.sizeof(limits)
        ))
        report = {
            "gui": api.current_facts(),
            "gui_in_source_job": api.in_job(api.kernel.GetCurrentProcess(), job),
            "gui_owned_source_job_limits": api.job_facts(job),
            "launches": {},
        }
        for name, flags in {"source": CREATE_SUSPENDED, **LAUNCH_MODES}.items():
            outcome = {"creation_flags": f"0x{flags:08x}"}
            report["launches"][name] = outcome
            try:
                process = api.launch("child", directory, name, flags)
            except OSError as error:
                outcome["create_error"] = error.winerror
                continue
            owned.append(process.process)
            try:
                if name == "source":
                    api.check(api.kernel.AssignProcessToJobObject(job, process.process))
                    if api.kernel.ResumeThread(process.thread) == 0xFFFFFFFF:
                        raise ct.WinError(ct.get_last_error())
            finally:
                api.kernel.CloseHandle(process.thread)
            outcome.update({
                "process_handle_in_gui": process.process,
                "pid_for_diagnostics_only": process.pid,
                "in_gui_owned_source_job": api.in_job(process.process, job),
                "child": read_json(directory / f"{name}.json", process.process, api),
            })
        write_json(directory / "gui.json", report)
        time.sleep(30)
    finally:
        api.kernel.CloseHandle(job)
        api.cleanup_all(owned)


def probe(api):
    print(json.dumps({
        "probe": "windows_guardian_launch",
        "controller": api.current_facts(),
        "job_query_scope": "immediate Job only; ancestor limits may also apply",
        "ctypes_sizes": {"startup": ct.sizeof(StartupInfo), "job_limits": ct.sizeof(ExtendedLimits)},
    }), flush=True)
    with tempfile.TemporaryDirectory(prefix="capture-windows-launch-") as temporary:
        directory = Path(temporary)
        parent = api.launch("gui", directory, "gui", 0)
        api.kernel.CloseHandle(parent.thread)
        owned = {}
        try:
            report = read_json(directory / "gui.json", parent.process, api)
            for name, outcome in report["launches"].items():
                if "create_error" in outcome:
                    continue
                handle = wt.HANDLE()
                api.check(api.kernel.DuplicateHandle(
                    parent.process, outcome.pop("process_handle_in_gui"),
                    api.kernel.GetCurrentProcess(), ct.byref(handle), 0, False, 2
                ))
                owned[name] = handle.value
            print(json.dumps({"before_gui_death": report}), flush=True)
            api.check(api.kernel.TerminateProcess(parent.process, 91))
            if api.kernel.WaitForSingleObject(parent.process, 5000) != WAIT_OBJECT_0:
                raise RuntimeError("fixture GUI did not exit within five seconds")
            source_state = api.kernel.WaitForSingleObject(owned["source"], 5000)
            after = {}
            for name, handle in owned.items():
                state = api.kernel.WaitForSingleObject(handle, 250)
                if state not in (WAIT_OBJECT_0, WAIT_TIMEOUT):
                    raise ct.WinError(ct.get_last_error())
                after[name] = {"alive_after_gui_death": state == WAIT_TIMEOUT}
            print(json.dumps({"after_gui_death": after}), flush=True)
            if (
                report["gui_in_source_job"]
                or not report["launches"]["source"]["in_gui_owned_source_job"]
                or source_state != WAIT_OBJECT_0
            ):
                raise RuntimeError("GUI-owned source Job control did not behave as configured")
            for name, outcome in report["launches"].items():
                if "create_error" not in outcome and name != "source":
                    if outcome["in_gui_owned_source_job"] or not after[name]["alive_after_gui_death"]:
                        raise RuntimeError(f"{name} did not survive outside the GUI-owned source Job")
        finally:
            api.cleanup_all([parent.process, *owned.values()])
    print(json.dumps({"owned_fixture_cleanup": "complete"}), flush=True)


if __name__ == "__main__":
    if sys.platform != "win32":
        raise SystemExit("This fixture probe requires Windows.")
    windows = Windows()
    if len(sys.argv) == 1:
        probe(windows)
    elif sys.argv[1] == "gui":
        gui(windows, Path(sys.argv[2]))
    elif sys.argv[1] == "child":
        child(windows, Path(sys.argv[2]), sys.argv[3])
    else:
        raise SystemExit("Unknown fixture role.")
