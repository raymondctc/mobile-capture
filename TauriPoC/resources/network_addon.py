"""Bounded original-byte evidence for the isolated Tauri capture prototype."""
import asyncio
import base64
import datetime as dt
import io
import json
import os
import threading
import time
from pathlib import Path
from mitmproxy import ctx, http, io as mitmio


def iso(timestamp=None):
    return dt.datetime.fromtimestamp(time.time() if timestamp is None else timestamp, dt.timezone.utc).isoformat().replace("+00:00", "Z")


def encoded(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def hold_writer(path):
    stream = open(path, "a+b")
    if os.name == "nt":
        import msvcrt
        stream.seek(0)
        if not stream.read(1):
            stream.write(b"\0")
            stream.flush()
        stream.seek(0)
        msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
    else:
        import fcntl
        fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
    return stream


class Body:
    def __init__(self):
        self.data = bytearray()
        self.complete = False
        self.unavailable = False

    def stream(self, chunk):
        if not chunk:
            self.complete = True
        elif not self.unavailable:
            if len(self.data) + len(chunk) > 256 * 1024:
                self.data.clear()
                self.unavailable = True
            else:
                self.data.extend(chunk)
        return chunk

    def finish(self, message):
        if not self.complete and not self.data and message.raw_content is not None:
            self.stream(message.raw_content)
            self.complete = True
        return bytes(self.data) if self.complete and not self.unavailable else None


class Capture:
    def __init__(self):
        self.config = json.loads(Path(os.environ["CAPTURE_ENGINE_CONFIG"]).read_text())
        self.session = Path(self.config["directory"])
        self.root = self.session / "network"
        self.runtime = self.session / "runtime"
        self.mode = self.config.get("forwarding", False)
        self.pending = {}
        self.admitted = 0
        self.omitted = 0
        self.tls_errors = 0
        self.missing = []
        self.errors = []
        self.decoded = 0
        self.total = 0
        self.closed = self.mode
        self.started = time.monotonic()
        self.writer_lease = None
        self.files = []
        self.finished = False
        self.initialized = False

    def running(self):
        self.runtime.mkdir(parents=True, exist_ok=True)
        try:
            self.writer_lease = hold_writer(self.runtime / ("recovery.writer" if self.mode else "network.writer"))
        except OSError:
            ctx.master.shutdown()
            return
        try:
            if not self.mode:
                self.root.mkdir(parents=True, exist_ok=True)
                self.native = open(self.root / "capture.mitm", "xb")
                self.files.append(self.native)
                self.events = open(self.root / "events.jsonl", "xb")
                self.files.append(self.events)
                self.spool = open(self.runtime / "har.entries", "x+b")
                self.files.append(self.spool)
        except OSError as error:
            self.errors.append(str(error)[:2048])
            ctx.master.shutdown()
            return
        self.initialized = True
        loop = asyncio.get_running_loop()
        def owner_eof():
            while os.read(0, 1):
                pass
            if not self.finished:
                loop.call_soon_threadsafe(ctx.master.shutdown)
        threading.Thread(target=owner_eof, daemon=True).start()
        asyncio.create_task(self.watch())
        (self.runtime / "engine-ready.json").write_bytes(encoded({"nonce": self.config["nonce"], "ready_at": iso()}))
        self.coverage()

    async def watch(self):
        while True:
            await asyncio.sleep(.05)
            stop = self.runtime / "engine-stop.json"
            try:
                if json.loads(stop.read_text()).get("nonce") == self.config["nonce"]:
                    ctx.master.shutdown()
                    return
            except (OSError, ValueError):
                pass
            if time.monotonic() - self.started >= 120:
                self.closed = True
            self.coverage()

    def coverage(self):
        if self.mode:
            return
        value = {"decoded_http_events": self.decoded, "tls_errors": self.tls_errors,
                 "omitted_flows": self.omitted, "unavailable_bodies": len(self.missing),
                 "admission_closed": self.closed}
        temporary = self.runtime / "coverage.pending"
        temporary.write_bytes(encoded(value))
        temporary.replace(self.runtime / "coverage.json")

    @staticmethod
    def headers(message):
        return [{"name": k, "value": v} for k, v in message.headers.items(multi=True)]

    def retainable_metadata(self, value):
        try:
            return len(encoded(value)) <= 16 * 1024
        except UnicodeError:
            if len(self.errors) < 32:
                self.errors.append("Metadata cannot be represented as UTF-8; flow omitted while original traffic forwards")
            return False

    def requestheaders(self, flow):
        flow.request.stream = lambda chunk: chunk
        if time.monotonic() - self.started >= 120 or self.admitted >= 32:
            self.closed = True
        if self.closed or not self.retainable_metadata({"headers": self.headers(flow.request), "method": flow.request.method, "url": flow.request.pretty_url, "version": flow.request.http_version}):
            self.omitted = min(self.omitted + 1, 2**63 - 1)
            return
        self.admitted += 1
        slot = {"flow": flow, "request": Body(), "response": Body()}
        self.pending[flow.id] = slot
        flow.request.stream = slot["request"].stream

    def responseheaders(self, flow):
        flow.response.stream = lambda chunk: chunk
        slot = self.pending.get(flow.id)
        if slot and not self.retainable_metadata(self.headers(flow.response)):
            self.pending.pop(flow.id)
            self.omitted = min(self.omitted + 1, 2**63 - 1)
            return
        if slot:
            flow.response.stream = slot["response"].stream

    def request(self, flow):
        nonce = self.config["setup_nonce"]
        prefix = "/mobile-capture-check/" + nonce
        if flow.request.host.lower() == "mitm.it" and flow.request.method == "GET" and flow.request.path in (prefix, prefix + "/https"):
            payload = {"nonce": nonce, "capture_check": "browser_https" if flow.request.scheme == "https" else "browser_http", "message": "Synthetic capture check: café 日本語"}
            flow.response = http.Response.make(200, encoded(payload), {"Content-Type": "application/json; charset=utf-8", "Cache-Control": "no-store"})
            flow.metadata["mobile_capture_setup_nonce"] = nonce

    def response(self, flow):
        self.persist(flow)

    def error(self, flow):
        if not self.mode and len(self.errors) < 32:
            self.errors.append("HTTP flow failed: " + str(flow.error)[:2048])
        self.persist(flow)

    @staticmethod
    def body_detail(body):
        if body is None:
            return {"encoding": None, "text": None, "bytes": None, "unavailable": True,
                    "reason": "Original streamed body unavailable; this is not an empty body", "byte_basis": "raw_content"}
        return {"encoding": "base64", "text": base64.b64encode(body).decode("ascii"), "bytes": len(body), "byte_basis": "raw_content"}

    def persist(self, flow, pending=False):
        slot = self.pending.pop(flow.id, None)
        if slot is None:
            return
        try:
            copy = flow.copy()
            copy.id = flow.id
            request = slot["request"].finish(flow.request)
            response = slot["response"].finish(flow.response) if flow.response else None
            copy.request.raw_content = request
            if copy.response:
                copy.response.raw_content = response
            bodies = [("request", request)] + ([("response", response)] if flow.response else [])
            for name, body in bodies:
                if body is None:
                    self.missing.append({"flow_id": flow.id, "source": name})
            detail = {"flow_id": flow.id, "method": flow.request.method, "url": flow.request.pretty_url,
                      "request_headers": self.headers(flow.request), "request_body": self.body_detail(request)}
            if flow.metadata.get("mobile_capture_setup_nonce"):
                detail.update(setup_probe=True, setup_probe_nonce=self.config["setup_nonce"], setup_probe_transport="android_reverse")
            events = [self.event(flow, "request", f"{flow.request.method} {flow.request.pretty_url}", detail, flow.request.timestamp_start)]
            entry = {"_flow_id": flow.id, "_incomplete": pending or flow.response is None,
                     "startedDateTime": iso(flow.request.timestamp_start), "time": max(0, (time.time() - flow.request.timestamp_start) * 1000),
                     "request": self.har_message(flow.request, request, True),
                     "response": self.har_message(flow.response, response, False) if flow.response else {"status": 0, "statusText": "Incomplete", "bodySize": -1, "content": {"_bodyUnavailable": True}},
                     "cache": {}, "timings": {"send": 0, "wait": -1, "receive": 0}}
            if flow.response:
                detail = dict(detail, status=flow.response.status_code, response_headers=self.headers(flow.response), response_body=self.body_detail(response))
                events.append(self.event(flow, "response", f"{flow.response.status_code} {flow.request.method} {flow.request.pretty_url}", detail, flow.response.timestamp_end or flow.response.timestamp_start))
            elif flow.error:
                entry["_error"] = str(flow.error)[:2048]
                events.append(self.event(flow, "error", "Request failed", dict(detail, error=str(flow.error)[:2048]), None))
            native = io.BytesIO()
            mitmio.FlowWriter(native).add(copy)
            raw, lines, har = native.getvalue(), b"".join(encoded(e) + b"\n" for e in events), encoded(entry) + b"\n"
            size = len(raw) + len(lines) + 2 * len(har)
            if self.total + size > 95 * 1024 * 1024:
                self.closed = True
                self.omitted = min(self.omitted + 1, 2**63 - 1)
                return
            self.native.write(raw)
            self.events.write(lines)
            self.spool.write(har)
            self.total += size
            self.decoded += len([e for e in events if e["kind"] in ("request", "response")])
            for file in self.files:
                file.flush()
        except Exception as error:
            self.closed = True
            if len(self.errors) < 32:
                self.errors.append(str(error)[:2048])

    def event(self, flow, kind, summary, detail, timestamp):
        return {"id": f"network:{flow.id}:{kind}", "timestamp": iso(timestamp),
                "original_timestamp": str(timestamp) if timestamp is not None else None, "received_at": iso(),
                "source": "network", "kind": kind, "summary": summary[:4096], "raw_ref": f"network/capture.mitm#{flow.id}",
                "detail": detail, "clock_uncertainty_ms": 1}

    def har_message(self, message, body, request):
        value = {"httpVersion": message.http_version, "headers": self.headers(message), "cookies": [], "headersSize": -1,
                 "bodySize": len(body) if body is not None else -1, "_byteBasis": "raw_content"}
        content = {"mimeType": message.headers.get("content-type", ""), **self.body_detail(body)}
        if request:
            value.update(method=message.method, url=message.pretty_url, queryString=[], postData=content)
        else:
            value.update(status=message.status_code, statusText=message.reason, redirectURL=message.headers.get("location", ""), content=dict(content, size=len(body) if body is not None else -1))
        if body is None:
            value["_bodyUnavailable"] = True
        return value

    def tls_failed_client(self, data):
        self.tls_errors = min(self.tls_errors + 1, 2**63 - 1)
        if not self.mode and self.tls_errors <= 32:
            if len(self.errors) < 32:
                self.errors.append("Client TLS handshake failed; check CA trust or pinning")
            event = {"id": f"network:tls:{self.tls_errors}", "timestamp": iso(), "received_at": iso(), "source": "network", "kind": "tls_error", "summary": "Client TLS handshake failed; check CA trust or pinning", "detail": {}, "clock_uncertainty_ms": 1}
            try:
                self.events.write(encoded(event) + b"\n")
                self.events.flush()
            except OSError as error:
                self.errors.append(str(error)[:2048])
                self.closed = True

    def done(self):
        if self.writer_lease is None:
            return
        self.finished = True
        pending_ids = list(self.pending)
        for slot in list(self.pending.values()):
            self.persist(slot["flow"], pending=True)
        try:
            if not self.initialized:
                return
            if not self.mode:
                self.spool.seek(0)
                with open(self.root / "capture.har", "xb") as har:
                    har.write(b'{"log":{"version":"1.2","creator":{"name":"Tauri capture prototype","version":"1"},"comment":"Bodies retain original raw_content bytes, including content encoding","entries":[')
                    first = True
                    for line in self.spool:
                        if not first:
                            har.write(b",")
                        har.write(line.rstrip(b"\n"))
                        first = False
                    har.write(b"]}}")
                    har.flush()
                    os.fsync(har.fileno())
                with open(self.root / "incomplete.json", "xb") as incomplete:
                    incomplete.write(encoded({"flow_ids": pending_ids, "count": len(pending_ids), "missing_bodies": self.missing, "omitted_flows": self.omitted, "errors": self.errors, "byte_basis": "raw_content", "policy": {"flows": 32, "body_bytes": 262144, "header_bytes": 16384, "artifact_bytes": 100663296, "duration_seconds": 120}}))
                    incomplete.flush()
                    os.fsync(incomplete.fileno())
            for file in self.files:
                file.flush()
                os.fsync(file.fileno())
                file.close()
            self.coverage()
            receipt = {"nonce": self.config["nonce"], "done": True, "partial": bool(pending_ids or self.missing or self.omitted or self.errors or self.tls_errors), "ended_at": iso()}
            target = self.runtime / "engine-receipt.json"
            with open(target, "xb") as file:
                file.write(encoded(receipt))
                file.flush()
                os.fsync(file.fileno())
        finally:
            for file in self.files:
                try:
                    file.close()
                except OSError:
                    pass
            self.writer_lease.close()


addons = [Capture()]
