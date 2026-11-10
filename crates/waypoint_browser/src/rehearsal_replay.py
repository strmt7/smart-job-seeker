"""Bounded recorded-response replay. No HTTP client, proxy, or live fallback.

This module runs in the existing controller of the zero-capability browser.
The container is the independent network boundary; Fetch only supplies recorded
responses. It does not prove all browser/broker channels are isolated.
"""
import base64
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
from urllib.parse import urlsplit, urlunsplit

MAX_BODY = 4 * 1024**2
MAX_TOTAL = 64 * 1024**2
INTERNAL_ID = 1_000_000_000


class ReplayError(ValueError):
    pass


def relative(value):
    if (not isinstance(value, str) or not value or len(value) > 512
            or re.search(r'[\\:*?"<>|\x00-\x1f]', value)):
        raise ReplayError('invalid_replay_path')
    parts = value.split('/')
    if any(not part or part in ('.', '..') or part.endswith(('.', ' '))
           or re.fullmatch(r'(?i)(con|prn|aux|nul|com[1-9¹²³]|lpt[1-9¹²³])(?:\..*)?', part)
           for part in parts):
        raise ReplayError('invalid_replay_path')
    return PurePosixPath(*parts)


def request_url(value):
    if not isinstance(value, str) or len(value) > 8192 or any(ord(c) < 33 for c in value):
        raise ReplayError('invalid_replay_url')
    try:
        parsed = urlsplit(value)
        if (parsed.scheme != 'https' or not parsed.hostname or parsed.username is not None
                or parsed.password is not None or parsed.port not in (None, 443)):
            raise ReplayError('invalid_replay_url')
        # Captures supply the browser's canonical URL, including query ordering.
        # Fragments never form part of an HTTP request/cache identity.
        return urlunsplit((parsed.scheme, parsed.netloc, parsed.path or '/', parsed.query, ''))
    except ValueError:
        raise ReplayError('invalid_replay_url') from None


def sha(value):
    return isinstance(value, str) and re.fullmatch('[a-f0-9]{64}', value) is not None


class Archive:
    def __init__(self, root, name, files, entry):
        self.root = Path(root)
        name = str(relative(name))
        if name not in files:
            raise ReplayError('replay_manifest_not_in_inventory')
        with (self.root / name).open('rb') as stream:
            data = stream.read(1024**2+1)
        if len(data) > 1024**2 or hashlib.sha256(data).hexdigest() != files[name]:
            raise ReplayError('replay_manifest_changed')
        self.manifest_sha256 = hashlib.sha256(data).hexdigest()
        def unique(pairs):
            result = {}
            for key, item in pairs:
                if key in result:
                    raise ReplayError('duplicate_replay_json_key')
                result[key] = item
            return result
        try:
            value = json.loads(data, object_pairs_hook=unique)
        except (RecursionError, UnicodeDecodeError, json.JSONDecodeError):
            raise ReplayError('invalid_replay_json') from None
        if (not isinstance(value, dict) or set(value) != {'schema', 'body_representation', 'entry_url', 'resources'}
                or type(value['schema']) is not int or value['schema'] != 1
                or value['body_representation'] != 'decoded_http'
                or not isinstance(value['resources'], list) or not 1 <= len(value['resources']) <= 512):
            raise ReplayError('invalid_replay_manifest')
        self.entry_url = value['entry_url']
        if not isinstance(self.entry_url,str) or len(self.entry_url)>4096:
            raise ReplayError('invalid_replay_entry')
        initial = request_url(self.entry_url)
        self.resources = {}
        total, checked = 0, {}
        required = {'url', 'method', 'request_body_sha256', 'status', 'headers', 'body_file', 'body_sha256'}
        for item in value['resources']:
            if (not isinstance(item, dict) or set(item) != required or not isinstance(item['method'], str)
                    or not re.fullmatch('[A-Z]{1,32}', item['method'])
                    or item['method'] in ('CONNECT', 'TRACE') or not sha(item['request_body_sha256'])
                    or type(item['status']) is not int or not 200 <= item['status'] <= 599 or item['status'] in (206,304)):
                raise ReplayError('invalid_replay_resource')
            url = request_url(item['url'])
            if url != item['url']:
                raise ReplayError('noncanonical_replay_resource_url')
            body_file = str(relative(item['body_file']))
            if body_file not in files or not sha(item['body_sha256']) or item['body_sha256'] != files[body_file]:
                raise ReplayError('replay_body_not_in_inventory')
            if body_file not in checked:
                checked[body_file] = len(self.body(body_file, item['body_sha256']))
                total += checked[body_file]
                if total > MAX_TOTAL:
                    raise ReplayError('replay_body_budget_exceeded')
            if ((item['status'] in (204, 205) or item['method'] == 'HEAD') and checked[body_file]
                    or item['method'] in ('GET', 'HEAD') and item['request_body_sha256'] != hashlib.sha256(b'').hexdigest()):
                raise ReplayError('invalid_replay_empty_body')
            headers = item['headers']
            if not isinstance(headers, list) or len(headers) > 128:
                raise ReplayError('invalid_replay_headers')
            normalized, names, head_length = [], set(), None
            for header in headers:
                if (not isinstance(header, dict) or set(header) != {'name', 'value'}
                        or not isinstance(header['name'], str) or not re.fullmatch(r"[!#$%&'*+.^_`|~0-9A-Za-z-]{1,256}", header['name'])
                        or not isinstance(header['value'], str) or len(header['value']) > 8192
                        or any(ord(c) < 32 or ord(c) == 127 for c in header['value'])):
                    raise ReplayError('invalid_replay_headers')
                key = header['name'].lower()
                if key in names or key in ('set-cookie', 'set-cookie2', 'authorization', 'proxy-authorization', 'www-authenticate', 'proxy-authenticate'):
                    raise ReplayError('credential_or_duplicate_replay_header')
                names.add(key)
                if item['method'] == 'HEAD' and key == 'content-length':
                    head_value = header['value'].strip()
                    if not re.fullmatch('[0-9]{1,20}', head_value) or int(head_value) > 2**63-1:
                        raise ReplayError('invalid_replay_headers')
                    head_length = head_value
                if key not in ('content-length', 'content-encoding', 'transfer-encoding', 'connection', 'keep-alive', 'trailer', 'upgrade'):
                    normalized.append(header)
            normalized.append({'name': 'Content-Length', 'value': head_length if head_length is not None else str(checked[body_file])})
            key = (item['method'], url, item['request_body_sha256'])
            if key in self.resources:
                raise ReplayError('ambiguous_replay_resource')
            self.resources[key] = {**item, 'headers': normalized}
        first = self.resources.get(('GET', initial, hashlib.sha256(b'').hexdigest()))
        if (not first or first['body_file'] != entry or not 200 <= first['status'] < 300
                or not any(h['name'].lower() == 'content-type' and h['value'].split(';')[0].lower() in ('text/html', 'application/xhtml+xml') for h in first['headers'])):
            raise ReplayError('invalid_replay_entry')

    def body(self, name, expected):
        path = self.root.joinpath(*relative(name).parts)
        if path.is_symlink() or not path.resolve().is_relative_to(self.root.resolve()) or path.stat().st_size > MAX_BODY:
            raise ReplayError('replay_body_budget_exceeded')
        with path.open('rb') as stream:
            data = stream.read(MAX_BODY + 1)
        if len(data) > MAX_BODY or hashlib.sha256(data).hexdigest() != expected:
            raise ReplayError('replay_body_changed')
        return data

    def response(self, request):
        method = request.get('method')
        if not isinstance(method, str) or not re.fullmatch('[A-Z]{1,32}', method):
            return None, 'unsupported_method'
        try:
            url = request_url(request.get('url'))
        except ReplayError:
            return None, 'unsupported_url'
        headers = request.get('headers', {})
        content_type = next((v for k, v in headers.items() if str(k).lower() == 'content-type'), '') if isinstance(headers, dict) else ''
        if (not isinstance(headers, dict) or not isinstance(content_type, str)
                or content_type.lower().startswith('multipart/')):
            return None, 'opaque_request_body'
        # CDP can include both a UTF-8 convenience string and byte entries.
        # Only complete, bounded byte entries are usable; file omissions and
        # multipart payloads are never reconstructed or guessed.
        data = None
        if 'postDataEntries' in request:
            entries = request['postDataEntries']
            if not isinstance(entries, list) or not 1 <= len(entries) <= 256:
                return None, 'opaque_request_body'
            pieces, size = [], 0
            for entry in entries:
                if (not isinstance(entry, dict) or set(entry) != {'bytes'}
                        or not isinstance(entry['bytes'], str) or len(entry['bytes']) > (MAX_BODY+2)//3*4):
                    return None, 'opaque_request_body'
                try:
                    piece = base64.b64decode(entry['bytes'], validate=True)
                except (ValueError, UnicodeEncodeError):
                    return None, 'opaque_request_body'
                size += len(piece)
                if size > MAX_BODY:
                    return None, 'opaque_request_body'
                pieces.append(piece)
            data = b''.join(pieces)
        if 'postData' in request:
            post = request['postData']
            if not isinstance(post, str) or len(post) > MAX_BODY:
                return None, 'opaque_request_body'
            try:
                text_data = post.encode('utf-8')
            except UnicodeEncodeError:
                return None, 'opaque_request_body'
            if len(text_data) > MAX_BODY or data is not None and data != text_data:
                return None, 'opaque_request_body'
            data = text_data
        if data is None:
            if request.get('hasPostData'):
                return None, 'opaque_request_body'
            data = b''
        item = self.resources.get((method, url, hashlib.sha256(data).hexdigest()))
        if item is None:
            return None, 'response_not_recorded'
        return {'responseCode': item['status'], 'responseHeaders': item['headers'],
                'body': base64.b64encode(self.body(item['body_file'], item['body_sha256'])).decode('ascii')}, 'recorded_response'


class Broker:
    """Services Fetch on the continuously pumped browser pipe, including children."""
    def __init__(self, archive, send, emit, discovery=False):
        if type(discovery) is not bool:
            raise ReplayError('invalid_discovery_mode')
        self.archive, self.send, self.emit = archive, send, emit
        self.discovery = discovery
        self.proposals, self.proposal_bytes, self.omitted_proposals, self.ineligible_requests = {}, 0, 0, 0
        self.next_id, self.pending, self.attaching, self.sessions, self.targets = INTERNAL_ID, {}, {}, {}, {}
        self.counts = {'served': 0, 'blocked': 0, 'cached_non_read_replies': 0, 'unreplayed_transports': 0, 'unmanaged_child_sessions':0}
        self.reasons, self.gaps = {}, {}

    def report(self):
        return {'schema': 1, 'mode': 'offline_response_replay', **self.counts,
                'blocked_reasons': dict(self.reasons), 'unsupported_targets': dict(self.gaps),
                'server_validation': 'not_performed', 'external_delivery': 'not_performed_by_replay'}

    def call(self, method, params, session, action):
        if len(self.pending) >= 128 or self.next_id >= 2**31-1:
            raise ReplayError('replay_command_budget_exceeded')
        self.next_id += 1
        self.pending[self.next_id] = action
        command = {'id': self.next_id, 'method': method, 'params': params}
        if session:
            command['sessionId'] = session
        self.send(command)

    def host(self, message):
        if message['id'] >= INTERNAL_ID or message['method'].startswith('Fetch.') or message['method'] in ('Target.setAutoAttach', 'Target.autoAttachRelated'):
            raise ReplayError('replay_control_override')
        if self.discovery:
            self.discovery_command(message)
        if message['method'] == 'SmartJobseeker.discoveryReport':
            if not self.discovery:
                raise ReplayError('not_resource_discovery')
            self.emit({'id': message['id'], 'result': {**self.report(), 'mode':'resource_discovery',
                'urls':sorted(self.proposals), 'omitted_requests':self.omitted_proposals,
                'ineligible_requests':self.ineligible_requests, 'pending_commands':len(self.pending)}})
            return False
        if message['method'] == 'SmartJobseeker.replayReport':
            self.emit({'id': message['id'], 'result': self.report()})
            return False
        if message['method'] == 'Target.attachToTarget':
            if len(self.attaching) >= 16:
                raise ReplayError('replay_target_budget_exceeded')
            target=message.get('params',{}).get('targetId')
            if not isinstance(target,str) or not target or len(target)>512 or message['id'] in self.attaching:
                raise ReplayError('invalid_replay_target')
            self.attaching[message['id']] = target
        return True

    def discovery_command(self, message):
        """No candidate entry, arbitrary script, upload, click or live navigation."""
        method, params = message['method'], message.get('params')
        if not isinstance(params, dict):
            raise ReplayError('discovery_command_refused')
        if method in ('Page.enable', 'Network.enable', 'Browser.getVersion',
                      'SmartJobseeker.discoveryReport', 'SmartJobseeker.replayReport') and not params:
            return
        if method == 'Target.createTarget' and params == {'url':'about:blank'}:
            return
        if method in ('Target.activateTarget','Target.attachToTarget'):
            keys = {'targetId'} | ({'flatten'} if method=='Target.attachToTarget' else set())
            target=params.get('targetId')
            if (set(params)==keys and isinstance(target,str) and 0<len(target)<=512
                    and not any(ord(c)<32 for c in target)
                    and (method!='Target.attachToTarget' or params['flatten'] is True)):
                return
        if method=='Page.navigate' and params=={'url':self.archive.entry_url}:
            return
        raise ReplayError('discovery_command_refused')

    def propose(self, request, reason):
        if reason!='response_not_recorded' or request.get('method')!='GET':
            self.ineligible_requests += 1
            return
        # Never forward request headers, bodies, cookies or authentication.
        if (request.get('hasPostData') or 'postData' in request or 'postDataEntries' in request
                or any(str(k).lower() in ('cookie','authorization','proxy-authorization') and v
                       for k,v in request.get('headers',{}).items())):
            self.ineligible_requests += 1
            return
        url=request_url(request['url'])
        if url in self.proposals:
            return
        if len(url)>4096 or len(self.proposals)>=128 or self.proposal_bytes+len(url.encode('utf-8'))>128*1024:
            self.omitted_proposals += 1
            return
        self.proposals[url]=True
        self.proposal_bytes+=len(url.encode('utf-8'))

    def bind(self, session, kind, original, paused, target):
        if not isinstance(session, str) or not session or len(session) > 512 or any(ord(c)<32 for c in session) or session in self.sessions:
            raise ReplayError('invalid_replay_session')
        if len(self.sessions) >= 64:
            raise ReplayError('replay_target_budget_exceeded')
        self.sessions[session] = kind
        if not isinstance(target,str) or not target or len(target)>512:
            raise ReplayError('invalid_replay_target')
        if target in self.targets:
            primary=self.targets[target]
            if paused:
                raise ReplayError('duplicate_auto_attached_target')
            if primary['ready']:
                self.emit(original)
            else:
                if len(primary['aliases'])>=16:
                    raise ReplayError('replay_target_budget_exceeded')
                primary['aliases'].append(original)
            return
        self.targets[target]={'session':session,'ready':False,'aliases':[]}
        action = ('fetch', session, kind, original, paused)
        if kind in ('page', 'iframe'):
            self.call('Fetch.enable', {'patterns': [{'urlPattern': '*', 'requestStage': 'Request'}]}, session, action)
        else:
            self.gaps[kind] = self.gaps.get(kind, 0) + 1
            self.after_fetch(session, kind, original, paused)

    def after_fetch(self, session, kind, original, paused):
        self.call('Target.setAutoAttach', {'autoAttach': True, 'waitForDebuggerOnStart': True, 'flatten': True},
                  session, ('auto', session, kind, original, paused))

    def finish(self, session, original):
        for target in self.targets.values():
            if target['session']==session:
                target['ready']=True
                if original is not None:
                    self.emit(original)
                for reply in target['aliases']:
                    self.emit(reply)
                target['aliases'].clear()
                return
        raise ReplayError('unbound_replay_session')

    def browser(self, message):
        request_id = message.get('id')
        if request_id is not None and type(request_id) is not int:
            raise ReplayError('invalid_replay_message')
        if type(request_id) is int and request_id >= INTERNAL_ID:
            action = self.pending.pop(request_id, None)
            if action is None:
                raise ReplayError('unexpected_replay_reply')
            name, session, kind, original, paused = action
            if 'error' in message:
                if name == 'auto' and kind not in ('page', 'iframe'):
                    self.counts['unmanaged_child_sessions'] += 1
                else:
                    raise ReplayError('replay_interception_failed')
            if name == 'reply':
                self.counts[kind] += 1
                if kind == 'blocked':
                    self.reasons[original] = self.reasons.get(original, 0) + 1
                elif original:
                    self.counts['cached_non_read_replies'] += 1
            elif name == 'fetch':
                self.after_fetch(session, kind, original, paused)
            elif name == 'auto':
                if paused:
                    self.call('Runtime.runIfWaitingForDebugger', {}, session, ('resume', session, kind, original, paused))
                else:
                    self.finish(session,original)
            elif name == 'resume':
                self.finish(session,original)
            return False
        if request_id in self.attaching:
            target=self.attaching.pop(request_id)
            if 'error' not in message:
                self.bind(message.get('result', {}).get('sessionId'), 'page', message, False,target)
                return False
        method = message.get('method')
        params = message.get('params', {})
        if method == 'Target.attachedToTarget' and params.get('waitingForDebugger') is True:
            kind = params.get('targetInfo', {}).get('type')
            if kind not in ('page', 'iframe', 'worker', 'service_worker', 'shared_worker'):
                raise ReplayError('unsupported_replay_target')
            self.bind(params.get('sessionId'), kind, message, True,params.get('targetInfo',{}).get('targetId'))
            return False
        if method in ('Network.webSocketCreated', 'Network.webTransportCreated'):
            self.counts['unreplayed_transports'] += 1
        if method == 'Fetch.requestPaused':
            session, paused_id = message.get('sessionId'), params.get('requestId')
            if session not in self.sessions or not isinstance(paused_id, str) or not paused_id or len(paused_id) > 512 or any(ord(c) < 32 for c in paused_id):
                raise ReplayError('invalid_replay_pause')
            request = params.get('request', {})
            if not isinstance(request, dict):
                raise ReplayError('invalid_replay_pause')
            result, reason = self.archive.response(request)
            if result is None:
                if self.discovery:
                    self.propose(request, reason)
                self.call('Fetch.failRequest', {'requestId': paused_id, 'errorReason': 'BlockedByClient'}, session, ('reply', session, 'blocked', reason, False))
            else:
                self.call('Fetch.fulfillRequest', {'requestId': paused_id, **result}, session, ('reply', session, 'served', request.get('method') not in ('GET', 'HEAD'), False))
            return False
        return True
