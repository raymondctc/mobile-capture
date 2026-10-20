"""Real 12.2.3 loopback proof, with private SSLContext trust and native replay."""
import base64
import gzip
import http.client
import http.server
import json
import os
from pathlib import Path
import socket
import shutil
import zipfile
import ssl
import subprocess
import sys
import tempfile
import threading
import time

engine = os.environ.get("CAPTURE_TEST_MITMDUMP")
if not engine or not Path(engine).is_absolute():
    raise SystemExit("CAPTURE_TEST_MITMDUMP must name installed mitmdump 12.2.3; this proof never skips")
root = Path(tempfile.mkdtemp(prefix="tauri-bounded-engine-proof-"))
nonce = "12ab" * 16
setup_nonce = "9c" * 16
request_body = b"\x00\xff\x80original request\x00"
encoded_response = gzip.compress(b"\x00\xff\x80encoded original response" * 8, mtime=0)
large = b"\xff" * (256 * 1024 + 1)
class Handler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        assert body == request_body
        self.respond(encoded_response, True)
    def do_GET(self):
        if self.path == "/binary-request-header":
            assert self.headers.get("X-Fixture") == "\xff"
        self.respond(large if self.path == "/large" else b"bounded", False)
    def respond(self, body, compressed):
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(len(body)))
        if self.path == "/binary-header":
            self.send_header("X-Fixture", "\xff")
        if compressed:
            self.send_header("Content-Encoding", "gzip")
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *args):
        pass
server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()

def start(name, blocked=False, addon=None, unready=False):
    session = root / name
    runtime = session / "runtime"
    runtime.mkdir(parents=True, mode=0o700)
    ca = root / "private-ca"
    ca.mkdir(exist_ok=True, mode=0o700)
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    held = None
    if blocked:
        held = open(runtime / "network.writer", "a+b")
        if os.name == "nt":
            import msvcrt
            held.write(b"\0")
            held.flush()
            held.seek(0)
            msvcrt.locking(held.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
    config = runtime / "config.json"
    config.write_text(json.dumps({"directory": str(session), "nonce": nonce, "setup_nonce": setup_nonce, "forwarding": False}))
    errors = open(session / "stderr.private", "wb")
    proc = subprocess.Popen([engine, "--quiet", "--listen-host", "127.0.0.1", "--listen-port", str(port), "--set", "confdir=" + str(ca), "--set", "connection_strategy=lazy", "--set", "upstream_cert=false", "-s", str(addon or Path(__file__).resolve().parents[1] / "resources/network_addon.py")], env=dict(os.environ, CAPTURE_ENGINE_CONFIG=str(config)), stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=errors)
    if blocked or unready:
        proc.stdin.close()
        assert proc.wait(timeout=25) == 0
        assert not (runtime / "engine-ready.json").exists()
        with socket.socket() as probe:
            assert probe.connect_ex(("127.0.0.1", port)) != 0
        if held:
            held.close()
        errors.close()
        return session
    deadline = time.monotonic() + 25
    while not (runtime / "engine-ready.json").exists():
        if proc.poll() is not None or time.monotonic() > deadline:
            raise RuntimeError((session / "stderr.private").read_text())
        time.sleep(.025)
    assert json.loads((runtime / "engine-ready.json").read_text())["nonce"] == nonce
    return session, ca, port, proc, errors

session, ca, port, proc, errors = start("main")
try:
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
    connection.request("POST", f"http://127.0.0.1:{server.server_port}/encoded", request_body)
    response = connection.getresponse()
    assert response.read() == encoded_response
    assert response.getheader("Content-Encoding") == "gzip"
    connection.close()
    context = ssl.create_default_context(cafile=str(ca / "mitmproxy-ca-cert.pem"))
    connection = http.client.HTTPSConnection("127.0.0.1", port, context=context, timeout=10)
    connection.set_tunnel("mitm.it", 443)
    connection.request("GET", "/mobile-capture-check/" + setup_nonce + "/https")
    response = connection.getresponse()
    assert response.status == 200
    assert json.loads(response.read())["nonce"] == setup_nonce
    connection.close()
    for path in ["/large"] + ["/bounded"] * 33:
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        connection.request("GET", f"http://127.0.0.1:{server.server_port}{path}")
        response = connection.getresponse()
        assert response.read() == (large if path == "/large" else b"bounded")
        connection.close()
    (session / "runtime/engine-stop.json").write_text(json.dumps({"nonce": "wrong"}))
    time.sleep(.1)
    assert proc.poll() is None
    (session / "runtime/engine-stop.json").write_text(json.dumps({"nonce": nonce}))
    assert proc.wait(timeout=10) == 0
    receipt = json.loads((session / "runtime/engine-receipt.json").read_text())
    assert receipt["done"] and receipt["partial"]
    har = json.loads((session / "network/capture.har").read_text())["log"]["entries"]
    events = [json.loads(line) for line in (session / "network/events.jsonl").read_text().splitlines()]
    replay = root / "native-reader.py"
    replay.write_text("""import base64, json, os
from pathlib import Path
from mitmproxy import io, ctx
class Reader:
    def running(self):
        with open(os.environ['CAPTURE_REPLAY_FILE'], 'rb') as source:
            flows=[{'id':f.id, 'request':None if f.request.raw_content is None else base64.b64encode(f.request.raw_content).decode(), 'response':None if f.response is None or f.response.raw_content is None else base64.b64encode(f.response.raw_content).decode()} for f in io.FlowReader(source).stream()]
        Path(os.environ['CAPTURE_REPLAY_RESULT']).write_text(json.dumps(flows))
        ctx.master.shutdown()
addons=[Reader()]
""")
    replay_result = root / "native-replay.json"
    subprocess.run([engine, "--quiet", "--no-server", "--set", "confdir=" + str(ca), "-s", str(replay)], env=dict(os.environ, CAPTURE_REPLAY_FILE=str(session / "network/capture.mitm"), CAPTURE_REPLAY_RESULT=str(replay_result)), check=True, timeout=15)
    native = json.loads(replay_result.read_text())
    assert len(har) == len(native) == 32
    assert {e["_flow_id"] for e in har} == {f["id"] for f in native}
    by_id = {flow["id"]: flow for flow in native}
    for entry in har:
        native_flow = by_id[entry["_flow_id"]]
        matching = [e for e in events if e["detail"]["flow_id"] == entry["_flow_id"]]
        for kind, message, field in [("request", entry["request"], "postData"), ("response", entry["response"], "content")]:
            body = message[field]
            expected = native_flow[kind]
            assert body["text"] == expected, (kind, entry["_flow_id"])
            for event in matching:
                detail = event["detail"].get(kind + "_body")
                if detail is not None:
                    assert detail["text"] == expected
                    assert detail["byte_basis"] == "raw_content"
            if expected is None:
                assert body["unavailable"] and message["_bodyUnavailable"]
    first = native[0]
    assert base64.b64decode(first["request"]) == request_body
    assert base64.b64decode(first["response"]) == encoded_response
    assert native[2]["response"] is None
    incomplete = json.loads((session / "network/incomplete.json").read_text())
    assert incomplete["omitted_flows"] == 4
    assert len(incomplete["missing_bodies"]) == 1
    assert sum(path.stat().st_size for path in (session / "network").iterdir()) <= 96 * 1024 * 1024
finally:
    if proc.poll() is None:
        proc.kill()
        proc.wait()
    errors.close()

second, _, _, proc, errors = start("owner-eof")
try:
    proc.stdin.close()
    assert proc.wait(timeout=10) == 0
    assert json.loads((second / "runtime/engine-receipt.json").read_text())["done"]
finally:
    if proc.poll() is None:
        proc.kill()
        proc.wait()
    errors.close()
blocked_session = start("source-lease-held-before-readiness", blocked=True)
fault_addon = root / "fault-addon.py"
fault_addon.write_text("""import importlib.util
spec=importlib.util.spec_from_file_location('bounded_base', BASE)
module=importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
class Broken:
    def __init__(self,file): self.file=file
    def write(self,data): raise OSError('fixture injected cross-file write failure')
    def __getattr__(self,name): return getattr(self.file,name)
class Fault(module.Capture):
    def running(self):
        super().running()
        self.events=Broken(self.events)
        self.files[1]=self.events
addons=[Fault()]
""".replace("BASE", repr(str(Path(__file__).resolve().parents[1] / "resources/network_addon.py"))))
fault, _, port, proc, errors = start("write-failure", addon=fault_addon)
try:
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
    connection.request("POST", f"http://127.0.0.1:{server.server_port}/encoded", request_body)
    assert connection.getresponse().read() == encoded_response
    connection.close()
    (fault / "runtime/engine-stop.json").write_text(json.dumps({"nonce": nonce}))
    assert proc.wait(timeout=10) == 0
    assert json.loads((fault / "runtime/engine-receipt.json").read_text())["partial"]
    facts = json.loads((fault / "network/incomplete.json").read_text())
    assert facts["errors"] == ["fixture injected cross-file write failure"]
    assert json.loads((fault / "network/capture.har").read_text())["log"]["entries"] == []
finally:
    if proc.poll() is None:
        proc.kill()
        proc.wait()
    errors.close()
for header_side in ["request", "response"]:
    header_session, _, header_port, header_proc, header_errors = start("binary-" + header_side + "-header")
    try:
        connection = http.client.HTTPConnection("127.0.0.1", header_port, timeout=10)
        url = f"http://127.0.0.1:{server.server_port}/" + ("binary-header" if header_side == "response" else "binary-request-header")
        connection.request("GET", url, headers={"X-Fixture": b"\xff"} if header_side == "request" else {})
        response = connection.getresponse()
        assert response.status == 200
        assert response.read() == b"bounded"
        if header_side == "response":
            assert response.getheader("X-Fixture") == "\xff"
        connection.close()
        (header_session / "runtime/engine-stop.json").write_text(json.dumps({"nonce": nonce}))
        assert header_proc.wait(timeout=10) == 0
        assert json.loads((header_session / "runtime/engine-receipt.json").read_text())["partial"]
        facts = json.loads((header_session / "network/incomplete.json").read_text())
        assert facts["omitted_flows"] == 1 and len(facts["errors"]) == 1
        assert (header_session / "network/events.jsonl").stat().st_size == 0
        assert json.loads((header_session / "network/capture.har").read_text())["log"]["entries"] == []
    finally:
        if header_proc.poll() is None:
            header_proc.kill()
            header_proc.wait()
        header_errors.close()

flush_addon = root / "flush-fault-addon.py"
flush_addon.write_text("""import importlib.util
from pathlib import Path
spec=importlib.util.spec_from_file_location('bounded_flush_base', BASE)
module=importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
class BrokenFlush:
    def __init__(self,file): self.file=file
    def flush(self): raise OSError('fixture injected source flush failure')
    def close(self):
        self.file.close()
        raise OSError('fixture injected source close failure')
    def __getattr__(self,name): return getattr(self.file,name)
class LeaseOrder:
    def __init__(self,file,capture): self.file,self.capture=file,capture
    def close(self):
        assert all(file.closed for file in self.capture.files), 'source lease released before writer close'
        self.file.close()
        (self.capture.runtime/'closed-before-lease-release').write_text('verified')
class Fault(module.Capture):
    def running(self):
        super().running()
        self.native=BrokenFlush(self.native)
        self.files[0]=self.native
        self.writer_lease=LeaseOrder(self.writer_lease,self)
addons=[Fault()]
""".replace("BASE", repr(str(Path(__file__).resolve().parents[1] / "resources/network_addon.py"))))
flush, _, port, proc, errors = start("flush-failure", addon=flush_addon)
try:
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
    connection.request("POST", f"http://127.0.0.1:{server.server_port}/encoded", request_body)
    assert connection.getresponse().read() == encoded_response
    connection.close()
    (flush / "runtime/engine-stop.json").write_text(json.dumps({"nonce": nonce}))
    assert proc.wait(timeout=10) == 0
    assert (flush / "runtime/closed-before-lease-release").read_text() == "verified"
    assert not (flush / "runtime/engine-receipt.json").exists()
    assert json.loads((flush / "network/incomplete.json").read_text())["errors"] == ["fixture injected source flush failure"]
finally:
    if proc.poll() is None:
        proc.kill()
        proc.wait()
    errors.close()

partial_addon = root / "partial-open-addon.py"
partial_addon.write_text("""import builtins, importlib.util
spec=importlib.util.spec_from_file_location('bounded_partial_base', BASE)
module=importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
def fail_spool(path,*args,**kwargs):
    if str(path).endswith('har.entries'): raise OSError('fixture injected partial writer initialization failure')
    return builtins.open(path,*args,**kwargs)
module.open=fail_spool
class LeaseOrder:
    def __init__(self,file,capture): self.file,self.capture=file,capture
    def close(self):
        assert len(self.capture.files)==2 and all(file.closed for file in self.capture.files), 'partly initialized writers outlived lease'
        self.file.close()
        (self.capture.runtime/'partial-writers-closed-before-lease-release').write_text('verified')
class Fault(module.Capture):
    def running(self):
        super().running()
        self.writer_lease=LeaseOrder(self.writer_lease,self)
addons=[Fault()]
""".replace("BASE", repr(str(Path(__file__).resolve().parents[1] / "resources/network_addon.py"))))
partial = start("partial-writer-initialization", addon=partial_addon, unready=True)
assert (partial / "runtime/partial-writers-closed-before-lease-release").read_text() == "verified"
assert not (partial / "runtime/engine-receipt.json").exists()

def owned_session(mode):
    base = root / ("rust-guardian-" + mode)
    tools = base / "tools"
    tools.mkdir(parents=True)
    project = Path(__file__).resolve().parents[1]
    binaries = project / "target/debug"
    suffix = ".exe" if os.name == "nt" else ""
    for name in ["fixture-network-owner", "capture-guardian", "collector-worker", "fake-adb"]:
        if not (binaries / (name + suffix)).is_file():
            raise RuntimeError("Build the real Rust proof first: cargo build -p capture-poc-core --bins --locked")
    adb = tools / ("fake-adb" + suffix)
    shutil.copy2(binaries / ("fake-adb" + suffix), adb)
    state_path = tools / "fake-adb-state.json"
    state_path.write_text(json.dumps({"settings": {}, "reverse": {}, "connected": True, "acknowledge_clear": True, "commands": []}))
    namespace = base / "private-namespace"
    config = base / "owner-config.json"
    config.write_text(json.dumps({"paths": {"output_root": str(base / "output"), "runtime_root": str(namespace)},
        "tools": {"adb": str(adb), "worker": str(binaries / ("collector-worker" + suffix)),
                  "guardian": str(binaries / ("capture-guardian" + suffix)), "mitmdump": engine,
                  "addon": str(flush_addon if mode == "flush-error" else project / "resources/network_addon.py")},
        "serial": "fixture-stream" if os.name == "nt" else "fixture-stop-clean"}))
    errors = open(base / "owner.stderr.private", "wb")
    owner = subprocess.Popen([str(binaries / ("fixture-network-owner" + suffix)), str(config)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors)
    try:
        line = owner.stdout.readline()
        if not line:
            raise RuntimeError((base / "owner.stderr.private").read_text())
        snapshot = json.loads(line)
        assert snapshot["phase"] == "recording", snapshot
        view = snapshot["session"]
        directory = Path(view["directory"])
        coverage = view["network"]["detail"]
        route = json.loads((directory / "runtime/routing.json").read_text())
        port = int(route["endpoint"].removeprefix("tcp:"))
        if mode in ("success", "flush-error"):
            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
            connection.request("POST", f"http://127.0.0.1:{server.server_port}/encoded", request_body)
            assert connection.getresponse().read() == encoded_response
            connection.close()
            if mode == "success":
                context = ssl.create_default_context(cafile=coverage["public_certificate"])
                connection = http.client.HTTPSConnection("127.0.0.1", port, context=context, timeout=10)
                connection.set_tunnel("mitm.it", 443)
                path = coverage["synthetic_check_url"].split("mitm.it", 1)[1] + "/https"
                connection.request("GET", path)
                assert connection.getresponse().status == 200
                connection.close()
        elif mode == "binary-header":
            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
            connection.request("GET", f"http://127.0.0.1:{server.server_port}/binary-request-header", headers={"X-Fixture": b"\xff"})
            response = connection.getresponse()
            assert response.status == 200
            response.read()
            connection.close()
        elif mode == "tls-error":
            connection = http.client.HTTPSConnection("127.0.0.1", port, context=ssl.create_default_context(), timeout=10)
            connection.set_tunnel("mitm.it", 443)
            try:
                connection.request("GET", "/untrusted-private-ca")
            except ssl.SSLCertVerificationError:
                pass
            else:
                raise AssertionError("Private CA was unexpectedly trusted")
            finally:
                connection.close()
        else:
            with socket.socket() as reservation:
                reservation.bind(("127.0.0.1", 0))
                absent_port = reservation.getsockname()[1]
            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
            connection.request("GET", f"http://127.0.0.1:{absent_port}/refused")
            assert connection.getresponse().status == 502
            connection.close()
        time.sleep(.2)
        owner.stdin.write(b"e")
        owner.stdin.flush()
        output, _ = owner.communicate(timeout=40)
        assert owner.returncode == 0, (base / "owner.stderr.private").read_text()
        result = json.loads(output)
        assert result["outcome"]["outcome"] == "ready", result
        completion = result["outcome"]["session"]["network"]["detail"]
        assert completion["routing_restored"] and completion["partial"] == (mode != "success"), completion
        assert not completion["forced"] and not completion["interrupted"]
        state = json.loads(state_path.read_text())
        assert state["settings"] == {} and state["reverse"] == {}
        if mode == "flush-error":
            assert not (directory / "runtime/engine-receipt.json").exists()
            assert (directory / "runtime/closed-before-lease-release").read_text() == "verified"
            assert "Network engine has no verified graceful completion" in completion["reasons"]
        else:
            receipt = json.loads((directory / "runtime/engine-receipt.json").read_text())
            assert receipt["done"] and receipt["partial"] == (mode != "success")
        with zipfile.ZipFile(result["archive"]) as archive:
            manifest = json.loads(archive.read("manifest.json"))
            metadata = json.dumps({"context": manifest["context"], "network": manifest["network_completion"]})
            assert "public_certificate" not in metadata and "synthetic_check_url" not in metadata
            assert str(namespace) not in metadata
            events = [json.loads(line) for line in archive.read("network/events.jsonl").splitlines()]
            if mode == "success":
                entries = json.loads(archive.read("network/capture.har"))["log"]["entries"]
                assert len(entries) == 2
                assert base64.b64decode(entries[0]["request"]["postData"]["text"]) == request_body
                assert base64.b64decode(entries[0]["response"]["content"]["text"]) == encoded_response
                replay_result = base / "native-replay.json"
                subprocess.run([engine, "--quiet", "--no-server", "--set", "confdir=" + str(namespace / "ca"), "-s", str(replay)], env=dict(os.environ, CAPTURE_REPLAY_FILE=str(directory / "network/capture.mitm"), CAPTURE_REPLAY_RESULT=str(replay_result)), check=True, timeout=15)
                native = json.loads(replay_result.read_text())
                assert {f["id"] for f in native} == {e["_flow_id"] for e in entries}
                assert base64.b64decode(native[0]["request"]) == request_body
                assert base64.b64decode(native[0]["response"]) == encoded_response
            else:
                assert manifest["package_state"] == "partial" and manifest["completeness"]["partial"]
                if mode == "flush-error":
                    assert json.loads(archive.read("network/incomplete.json"))["errors"] == ["fixture injected source flush failure"]
                elif mode == "binary-header":
                    assert manifest["completeness"]["omitted_flows"] == 1
                    assert manifest["sources"]["network"] == "started"
                    assert manifest["completeness"]["sources"]["network"]["decoded_http_events"] == 0
                    assert json.loads(archive.read("network/incomplete.json"))["errors"]
                else:
                    kind = "tls_error" if mode == "tls-error" else "error"
                    assert any(event["kind"] == kind for event in events), events
                    assert manifest["completeness"]["network_error_refs"]
                    if mode == "tls-error":
                        assert manifest["sources"]["network"] == "tls_failed"
            if os.name != "nt":
                logs = manifest["collector_status"]
                assert logs["reason"] == "lease_closed" and not logs["forced"] and not logs["stderr"]
                assert not manifest["completeness"]["unparsed_event_refs"]
                assert manifest["package_state"] == ("complete" if mode == "success" else "partial")
        return {"mode": mode, "archive": result["archive"], "routing_restored": True, "network_partial": completion["partial"]}
    finally:
        if owner.poll() is None:
            owner.kill()
            owner.wait()
        errors.close()

owned_proofs = [owned_session(mode) for mode in ["success", "tls-error", "http-error", "flush-error", "binary-header"]]
server.shutdown()
result = {"root": str(root), "real_engine": "12.2.3", "native_har_jsonl_ids_and_raw_bytes_match": True, "http_binary_gzip_forwarded": True, "https_private_client_trust": True, "admitted_flows": 32, "omitted_flows": 4, "oversize_body_unavailable_everywhere": True, "private_nonce_shutdown": True, "owner_eof_graceful_done": True, "source_lease_failure_stops_unready_listener": True, "cross_file_write_failure_truthfully_partial": True, "flush_failure_closes_writers_before_source_lease_release": True, "partial_writer_initialization_closes_before_lease_release": True, "binary_headers_forward_but_decline_retention_truthfully": True, "device_routing": "compiled_private_fake_only", "rust_owner_guardian_real_engine": owned_proofs}
(root / "result.json").write_text(json.dumps(result, indent=2))
print(json.dumps(result))
