import base64
import datetime
import gzip
import hashlib
import json
from pathlib import Path

root = Path('/private/tmp/inspector-ux-native-fixture')
root.mkdir(exist_ok=True)
for name in ['logs', 'network', 'timeline']:
    (root / name).mkdir(exist_ok=True)

started = '2026-10-25T03:00:00.000000Z'
metadata = {
    'version': 1, 'id': 'inspector-ux-native-fixture',
    'device': {'serial': 'fixture-phone', 'name': 'UI acceptance fixture', 'state': 'ready'},
    'started_at': started, 'selection': {'mode': 'logs_and_network'},
    'context': {'title': 'Inspector UX acceptance fixture', 'notes': 'Synthetic local evidence.', 'steps': []},
}
(root / 'capture.json').write_text(json.dumps(metadata))

def body(value):
    return {'encoding': 'base64', 'text': base64.b64encode(value).decode(),
            'bytes': len(value), 'byte_basis': 'raw_content'}

request = b'{"action":"needle","limit":1e+09,"note":"caf\xc3\xa9"}'
response = b'{"10":"ten","2":"two","large":9007199254740993,"dup":1,"dup":2,"sentinel":"needle","nested":{"ok":true}}'
cases = [
    ('lexical-json', response, []),
    ('large-tail', b'{"padding":"' + b'a' * 70000 + b'","tail":"needle-tail"}', []),
    ('compressed-json', gzip.compress(response), [{'name': 'Content-Encoding', 'value': 'gzip'}]),
    ('empty-response', b'', []),
    ('binary-response', bytes([0, 255, 65, 66]), []),
    ('missing-response', None, []),
    ('head-empty', b'', [{'name': 'Content-Encoding', 'value': 'gzip'}]),
]
events = []
for index, (name, content, headers) in enumerate(cases):
    detail = {'method': 'HEAD' if name == 'head-empty' else ('POST' if index % 2 == 0 else 'GET'),
              'url': 'https://fixture.invalid/' + name,
              'request_headers': [{'name': 'Content-Type', 'value': 'application/json'}],
              'request_body': body(request), 'duration_ms': 125.4}
    if content is not None:
        detail.update(status=200 if index % 2 == 0 else 503,
                      response_headers=headers, response_body=body(content))
    events.append({'id': name, 'timestamp': f'2026-10-25T03:00:{index + 1:02d}.000000Z',
                   'kind': 'http_response' if content is not None else 'http_request',
                   'summary': detail['method'] + ' ' + detail['url'],
                   'raw_ref': 'network/events.jsonl', 'detail': detail})
(root / 'network/events.jsonl').write_text(''.join(json.dumps(event) + '\n' for event in events))
epoch = datetime.datetime.fromisoformat(started.replace('Z', '+00:00')).timestamp()
logs = ''.join(f'{epoch + 10 + index:.6f} 123 456 {level} PaymentApi: {message}\n'
               for index, (level, message) in enumerate([
                   ('D', 'request preparation'), ('I', 'connection ready'),
                   ('W', 'retry requested'), ('E', 'needle payment failed'),
                   ('F', 'fixture fatal detail'),
               ])) + 'Unparsed fixture log without a level\n'
(root / 'logs/device.log').write_text(logs)
(root / 'timeline/markers.jsonl').write_text(json.dumps({
    'timestamp': '2026-10-25T03:00:20.000000Z', 'text': 'Inspector verification marker',
}) + '\n')
digest = hashlib.sha256()
for file in sorted(path for path in root.rglob('*') if path.is_file()):
    digest.update(str(file.relative_to(root)).encode())
    digest.update(file.read_bytes())
Path('/private/tmp/inspector-ux-native-fixture.sha256').write_text(digest.hexdigest())
print(json.dumps({'fixture': str(root), 'sha256': digest.hexdigest(), 'events': len(events) + 7}))

heavy = Path('/private/tmp/inspector-ux-worker-fixture')
for name in ['', 'logs', 'network', 'timeline']:
    (heavy / name).mkdir(exist_ok=True)
heavy_metadata = dict(metadata, id='inspector-ux-worker-fixture',
                      context={'title': 'Inspector worker acceptance fixture', 'notes': 'Synthetic local evidence.', 'steps': []})
(heavy / 'capture.json').write_text(json.dumps(heavy_metadata))
flood = b'{"flood":"' + b'a' * 1048576 + b'"}'
heavy_event = {'id': 'worker-flood', 'timestamp': started, 'kind': 'http_response',
               'summary': 'POST https://fixture.invalid/worker-flood', 'detail': {
                   'method': 'POST', 'url': 'https://fixture.invalid/worker-flood', 'status': 200,
                   'request_body': body(flood), 'response_body': body(flood), 'duration_ms': 42}}
(heavy / 'network/events.jsonl').write_text(json.dumps(heavy_event) + '\n')
(heavy / 'logs/device.log').write_text('')
(heavy / 'timeline/markers.jsonl').write_text('')
heavy_digest = hashlib.sha256()
for file in sorted(path for path in heavy.rglob('*') if path.is_file()):
    heavy_digest.update(str(file.relative_to(heavy)).encode())
    heavy_digest.update(file.read_bytes())
Path('/private/tmp/inspector-ux-worker-fixture.sha256').write_text(heavy_digest.hexdigest())
print(json.dumps({'fixture': str(heavy), 'sha256': heavy_digest.hexdigest(), 'expected_a_matches': 2097152}))
