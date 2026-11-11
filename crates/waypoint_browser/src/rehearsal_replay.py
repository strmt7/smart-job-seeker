"""Bounded recorded-response replay. No HTTP client, proxy, or live fallback.

This module runs in the existing controller of the zero-capability browser.
The container is the independent network boundary; Fetch only supplies recorded
responses. It does not prove all browser/broker channels are isolated.
"""
import base64
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path, PurePosixPath
import re
from urllib.parse import urljoin, urlsplit, urlunsplit

MAX_BODY = 4 * 1024**2
MAX_TOTAL = 64 * 1024**2
INTERNAL_ID = 1_000_000_000
MAX_RETIRED_SESSIONS = 4096
MAX_RETIRED_REPLIES = 512


class ReplayError(ValueError):
    pass


def public_form_outline(result):
    """Return labels/context without node value properties, link targets or handles."""
    base = {'schema': 1, 'scope': 'root_frame_accessibility_tree', 'status': 'unavailable',
            'reason': None, 'nodes': 0, 'controls': [], 'omitted_controls': 0,
            'unlabelled_controls': 0, 'omitted_texts': 0}
    nodes = result.get('nodes') if isinstance(result, dict) else None
    if not isinstance(nodes, list) or not nodes or len(nodes) > 16384:
        return {**base, 'reason': 'node_budget_or_invalid_tree'}
    base['nodes'] = len(nodes)
    by_id = {}
    for node in nodes:
        key = node.get('nodeId') if isinstance(node, dict) else None
        if (not isinstance(key, str) or not 0 < len(key) <= 128 or key in by_id
                or any(ord(c) < 32 for c in key)):
            return {**base, 'reason': 'invalid_node_identity'}
        by_id[key] = node
    roles = {'textbox', 'searchbox', 'combobox', 'listbox', 'checkbox', 'radio',
             'switch', 'spinbutton', 'slider', 'button', 'link'}
    eligible = []
    for node in nodes:
        role = node.get('role', {})
        role = role.get('value') if isinstance(role, dict) else None
        if node.get('ignored') is False and isinstance(role, str) and role in roles:
            eligible.append((role, node))
    # Question controls precede navigation links, without interpreting intent.
    eligible.sort(key=lambda item: 2 if item[0] == 'link' else 1 if item[0] == 'button' else 0)

    def text(value, limit):
        raw = value.get('value') if isinstance(value, dict) else None
        if raw is None or raw == '':
            return None
        if isinstance(raw, str):
            normalized = ' '.join(raw.split())
            try:
                if (normalized and len(normalized.encode('utf-8')) <= limit
                        and not any(ord(c) < 32 or ord(c) == 127 for c in normalized)):
                    return normalized
            except UnicodeError:
                pass
        base['omitted_texts'] += 1
        return None

    used_bytes = 0
    for position, (role, node) in enumerate(eligible):
        if position >= 256:
            base['omitted_controls'] += 1
            continue
        properties = node.get('properties', [])
        if not isinstance(properties, list) or len(properties) > 64:
            return {**base, 'status': 'unavailable', 'controls': [], 'reason': 'property_budget_or_invalid_tree'}
        flags = {'required': None, 'disabled': None}
        for key in flags:
            values = [p.get('value', {}).get('value') for p in properties if isinstance(p, dict)
                      and p.get('name') == key and isinstance(p.get('value'), dict)]
            if values and all(type(v) is bool for v in values) and all(v == values[0] for v in values):
                flags[key] = values[0]
        context, visited, parent = [], {node['nodeId']}, node.get('parentId')
        incomplete = False
        for _ in range(64):
            if parent is None or len(context) == 3:
                break
            if not isinstance(parent, str) or parent in visited or parent not in by_id:
                incomplete = True
                break
            visited.add(parent)
            ancestor = by_id[parent]
            ancestor_role = ancestor.get('role', {})
            if isinstance(ancestor_role, dict) and ancestor_role.get('value') in ('group', 'radiogroup', 'form'):
                label = text(ancestor.get('name'), 400)
                if label and label not in context:
                    context.append(label)
            parent = ancestor.get('parentId')
        else:
            incomplete = True
        item = {'number': len(base['controls']) + 1, 'role': role,
                'label': text(node.get('name'), 400), 'description': text(node.get('description'), 600),
                'context': list(reversed(context)), 'context_incomplete': incomplete, **flags}
        size = len(json.dumps(item, ensure_ascii=False, separators=(',', ':')).encode('utf-8'))
        if used_bytes + size > 65536:
            base['omitted_controls'] += 1
            continue
        used_bytes += size
        base['unlabelled_controls'] += item['label'] is None
        base['controls'].append(item)
    partial = (base['omitted_controls'] or base['unlabelled_controls'] or base['omitted_texts']
               or any(c['context_incomplete'] for c in base['controls']))
    return {**base, 'status': 'partial' if partial else 'observed'}


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


class FetchPreloads(HTMLParser):
    """Project unambiguous head declarations; never fetch or inspect form values."""
    def __init__(self, url):
        super().__init__(convert_charrefs=True)
        self.base, self.base_seen = url, False
        self.in_head, self.head_seen, self.inert = False, False, 0
        self.urls, self.bytes = set(), 0

    def handle_starttag(self, tag, attrs):
        if tag == 'body':
            self.in_head, self.head_seen = False, True
            return
        if tag == 'head' and not self.head_seen:
            self.in_head = self.head_seen = True
        if not self.in_head:
            return
        if tag in ('template', 'noscript'):
            self.inert += 1
        if self.inert or tag not in ('link', 'base'):
            return
        values = dict(attrs)
        if len(values) != len(attrs):
            if tag == 'base': self.base, self.base_seen = None, True
            return
        href = values.get('href')
        if not isinstance(href, str) or not href or len(href) > 4096:
            return
        try:
            url = request_url(urljoin(self.base or '', href))
        except ReplayError:
            if tag == 'base' and not self.base_seen: self.base, self.base_seen = None, True
            return
        if tag == 'base' and not self.base_seen:
            self.base, self.base_seen = url, True
            return
        mime = values.get('type') or ''
        if ('preload' not in (values.get('rel') or '').lower().split() or (values.get('as') or '').lower() != 'fetch'
                or mime and not (mime.split(';')[0].strip().lower() == 'application/json'
                                or mime.split(';')[0].strip().lower().startswith('application/') and mime.split(';')[0].strip().lower().endswith('+json'))):
            return
        if url not in self.urls and len(self.urls) < 128 and self.bytes + len(url.encode()) <= 128 * 1024:
            self.urls.add(url); self.bytes += len(url.encode())

    def handle_endtag(self, tag):
        if tag in ('template', 'noscript') and self.inert: self.inert -= 1
        if tag == 'head': self.in_head = False


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
            if (not isinstance(item, dict) or not required.issubset(item)
                    or set(item) - required - {'request_origin_absent'} or not isinstance(item['method'], str)
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
            vary = next((h['value'] for h in normalized if h['name'].lower() == 'vary'), None)
            fields = {v.strip().lower() for v in vary.split(',')} if vary is not None else set()
            bound_origin = item.get('request_origin_absent') is True
            if (fields - {'accept-encoding', 'origin'} or ('origin' in fields) != bound_origin
                    or 'request_origin_absent' in item and not bound_origin
                    or bound_origin and (item['method'] != 'GET' or not any(
                        h['name'].lower() == 'content-type' and h['value'].split(';')[0].strip().lower() in ('text/html', 'application/xhtml+xml')
                        for h in normalized))):
                raise ReplayError('unbound_replay_variant')
            key = (item['method'], url, item['request_body_sha256'])
            if key in self.resources:
                raise ReplayError('ambiguous_replay_resource')
            self.resources[key] = {**item, 'headers': normalized}
        self.fetch_preloads = set()
        self.preload_projection_failures = 0
        preload_bytes = 0
        for item in self.resources.values():
            if item['method'] != 'GET' or not any(h['name'].lower() == 'content-type' and h['value'].split(';')[0].strip().lower() == 'text/html' for h in item['headers']):
                continue
            parser = FetchPreloads(item['url'])
            try:
                parser.feed(self.body(item['body_file'], item['body_sha256']).decode('utf-8-sig'))
            except (UnicodeError, ValueError, AssertionError):
                self.preload_projection_failures += 1
                continue
            for url in sorted(parser.urls):
                size = len(url.encode())
                if url not in self.fetch_preloads and len(self.fetch_preloads) < 128 and preload_bytes + size <= 128 * 1024:
                    self.fetch_preloads.add(url); preload_bytes += size
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

    def response(self, request, resource_type=None):
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
        if item.get('request_origin_absent'):
            # GET document navigation has no Fetch-generated Origin. Other
            # resource types can add Origin in the network stack after a pause;
            # their apparent absence is insufficient evidence for this variant.
            names = [key.lower() for key in headers if isinstance(key, str)]
            if (resource_type != 'Document' or 'origin' in names or len(headers) > 128
                    or len(names) != len(headers) or len(set(names)) != len(names)
                    or any(not re.fullmatch(r"[!#$%&'*+.^_`|~0-9A-Za-z-]{1,256}", key)
                           or not isinstance(value, str) or len(value) > 8192
                           or any(ord(c) < 32 or ord(c) == 127 for c in value)
                           for key, value in headers.items())):
                return None, 'origin_variant_not_recorded'
        return {'responseCode': item['status'], 'responseHeaders': item['headers'],
                'body': base64.b64encode(self.body(item['body_file'], item['body_sha256'])).decode('ascii')}, 'recorded_response'


class Broker:
    """Services Fetch on the continuously pumped browser pipe, including children."""
    def __init__(self, archive, send, emit, discovery=False):
        if type(discovery) is not bool:
            raise ReplayError('invalid_discovery_mode')
        self.archive, self.send, self.emit = archive, send, emit
        self.discovery = discovery
        self.discovery_session, self.discovery_frames = None, set()
        self.proposals, self.proposal_bytes, self.omitted_proposals, self.ineligible_requests = {}, 0, 0, 0
        self.asset_proposals = {}
        self.next_id, self.pending, self.attaching, self.sessions, self.targets = INTERNAL_ID, {}, {}, {}, {}
        self.session_targets, self.session_parents = {}, {}
        self.retired_sessions, self.retired_replies = set(), set()
        self.counts = {'served': 0, 'blocked': 0, 'cached_non_read_replies': 0, 'unreplayed_transports': 0, 'unmanaged_child_sessions':0}
        self.counts.update(discarded_replies=0, discarded_events=0, closed_aliases=0)
        self.reasons, self.gaps = {}, {}
        self.outline_requested, self.outline = False, None

    def report(self):
        return {'schema': 1, 'mode': 'offline_response_replay', **self.counts,
                'active_sessions':len(self.sessions), 'active_targets':len(self.targets),
                'retired_sessions':len(self.retired_sessions), 'retired_commands':len(self.retired_replies),
                'pending_detachments':sum(action[0]=='detach' for action in self.pending.values()),
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
        if isinstance(message.get('sessionId'), str) and message['sessionId'] in self.retired_sessions:
            self.emit(self.stale_reply(message))
            return False
        if self.discovery:
            self.discovery_command(message)
            if message['method'] == 'Page.navigate':
                session = message.get('sessionId')
                if self.discovery_session is not None or self.sessions.get(session) != 'page':
                    raise ReplayError('discovery_navigation_scope_refused')
                self.discovery_session = session
        if message['method'] == 'SmartJobseeker.publicFormOutline':
            if not self.discovery:
                raise ReplayError('not_resource_discovery')
            session = message.get('sessionId')
            if not isinstance(session, str) or self.sessions.get(session) != 'page' or self.outline_requested:
                raise ReplayError('outline_session_or_budget_refused')
            self.outline_requested = True
            self.call('Accessibility.getFullAXTree', {}, session, ('outline', session, 'page', message['id'], False))
            return False
        if message['method'] == 'SmartJobseeker.discoveryReport':
            if not self.discovery:
                raise ReplayError('not_resource_discovery')
            self.emit({'id': message['id'], 'result': {**self.report(), 'mode':'resource_discovery',
                'urls':sorted(self.proposals),
                'child_documents':sorted(url for url, child in self.proposals.items() if child),
                'assets':{url:sorted(kinds) for url,kinds in sorted(self.asset_proposals.items())},
                'preload_projection_failures':self.archive.preload_projection_failures,
                'omitted_requests':self.omitted_proposals,
                'ineligible_requests':self.ineligible_requests, 'pending_commands':len(self.pending),
                'form_outline': self.outline}})
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
            self.attaching[message['id']] = (target, message.get('sessionId'))
        return True

    @staticmethod
    def stale_reply(message):
        reply = {'id':message['id'], 'error':{'code':-32000,'message':'Replay session detached; inspect the current page again'}}
        if message.get('sessionId'):
            reply['sessionId'] = message['sessionId']
        return reply

    def retire(self, session):
        """Invalidate this interceptor's aliases/descendants before releasing state.

        Old acknowledgements are consumed separately, never counted as service or
        used to resume a newly attached target. No request gains a live fallback.
        """
        removed = {session}
        while True:
            previous = len(removed)
            removed.update(s for s, parent in self.session_parents.items() if parent in removed)
            primary_targets = {self.session_targets[s] for s in removed
                               if self.targets.get(self.session_targets[s],{}).get('session') == s}
            removed.update(s for s, target in self.session_targets.items() if target in primary_targets)
            if len(removed) == previous:
                break
        commands = {key:action for key,action in self.pending.items() if action[1] in removed}
        if (len(self.retired_sessions) + len(removed) > MAX_RETIRED_SESSIONS
                or len(self.retired_replies) + len(commands) > MAX_RETIRED_REPLIES):
            raise ReplayError('replay_lifecycle_budget_exceeded')
        self.retired_sessions.update(removed)
        self.retired_replies.update(commands)
        for key, action in commands.items():
            del self.pending[key]
            name, scoped, _, original, _ = action
            if name == 'outline':
                self.outline = {**public_form_outline(None),'reason':'browser_target_detached'}
                self.emit({'id':original,'sessionId':scoped,'result':self.outline})
            elif name in ('fetch','auto','resume') and isinstance(original,dict) and 'id' in original:
                self.emit(self.stale_reply(original))
        for target, state in list(self.targets.items()):
            kept = []
            for reply in state['aliases']:
                if reply['result']['sessionId'] in removed:
                    self.emit(self.stale_reply(reply))
                else:
                    kept.append(reply)
            state['aliases'] = kept
            if state['session'] in removed:
                del self.targets[target]
        for scoped in removed:
            del self.sessions[scoped]
            del self.session_targets[scoped]
            del self.session_parents[scoped]
        # Losing an interceptor need not destroy its underlying target. Close
        # invalidated aliases/descendants too, so fresh attachment cannot leave
        # real browser sessions resident behind retired bookkeeping entries.
        for scoped in sorted(removed - {session}):
            self.call('Target.detachFromTarget', {'sessionId':scoped}, None,
                      ('detach', None, None, None, False))

    def discovery_command(self, message):
        """No candidate entry, arbitrary script, upload, click or live navigation."""
        method, params = message['method'], message.get('params')
        if not isinstance(params, dict):
            raise ReplayError('discovery_command_refused')
        if method in ('Page.enable', 'Network.enable', 'Browser.getVersion',
                      'SmartJobseeker.discoveryReport', 'SmartJobseeker.replayReport',
                      'SmartJobseeker.publicFormOutline') and not params:
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

    def propose(self, request, reason, child_document, asset_kind=None):
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
            self.proposals[url] |= child_document
            if asset_kind is not None:self.asset_proposals.setdefault(url,set()).add(asset_kind)
            return
        if len(url)>4096 or len(self.proposals)>=128 or self.proposal_bytes+len(url.encode('utf-8'))>128*1024:
            self.omitted_proposals += 1
            return
        self.proposals[url]=child_document
        self.proposal_bytes+=len(url.encode('utf-8'))
        if asset_kind is not None:self.asset_proposals[url]={asset_kind}

    def bind(self, session, kind, original, paused, target, parent=None):
        if not isinstance(session, str) or not session or len(session) > 512 or any(ord(c)<32 for c in session) or session in self.sessions or session in self.retired_sessions:
            raise ReplayError('invalid_replay_session')
        if len(self.sessions) >= 64:
            raise ReplayError('replay_target_budget_exceeded')
        if not isinstance(target,str) or not target or len(target)>512:
            raise ReplayError('invalid_replay_target')
        if parent is not None and parent not in self.sessions:
            raise ReplayError('unknown_replay_parent')
        self.sessions[session] = kind
        self.session_targets[session], self.session_parents[session] = target, parent
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
            if request_id in self.retired_replies:
                self.retired_replies.remove(request_id)
                self.counts['discarded_replies'] += 1
                return False
            action = self.pending.pop(request_id, None)
            if action is None:
                raise ReplayError('unexpected_replay_reply')
            name, session, kind, original, paused = action
            if name == 'detach':
                error = message.get('error')
                if error is not None and not (isinstance(error,dict)
                        and error.get('code') in (-32000,-32602)
                        and error.get('message') == 'No session with given id'):
                    raise ReplayError('replay_alias_cleanup_failed')
                self.counts['closed_aliases'] += 1
                return False
            if name == 'outline':
                self.outline = public_form_outline(None if 'error' in message else message.get('result'))
                if 'error' in message:
                    self.outline['reason'] = 'browser_read_unavailable'
                self.emit({'id': original, 'sessionId': session, 'result': self.outline})
                return False
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
            target, parent=self.attaching.pop(request_id)
            if parent in self.retired_sessions:
                self.emit(self.stale_reply(message))
                return False
            if 'error' not in message:
                self.bind(message.get('result', {}).get('sessionId'), 'page', message, False,target,parent)
                return False
        method = message.get('method')
        if method == 'Target.detachedFromTarget':
            params = message.get('params',{})
            detached = params.get('sessionId') if isinstance(params,dict) else None
            if not isinstance(detached,str) or not detached or len(detached)>512 or any(ord(c)<32 for c in detached):
                raise ReplayError('invalid_replay_detachment')
            if detached in self.sessions:
                self.retire(detached)
                return True
            if detached in self.retired_sessions:
                self.counts['discarded_events'] += 1
                return False
        scoped = message.get('sessionId')
        if isinstance(scoped,str) and scoped in self.retired_sessions:
            if request_id is not None:
                self.emit(self.stale_reply(message))
            else:
                self.counts['discarded_events'] += 1
            return False
        if self.discovery and isinstance(method, str) and method.startswith('Accessibility.'):
            return False  # Raw accessibility events may contain control values.
        params = message.get('params', {})
        if self.discovery and scoped is not None and scoped == self.discovery_session:
            frame = params.get('frameId')
            if isinstance(frame, str) and 0 < len(frame) <= 512:
                if method == 'Page.frameAttached' and len(self.discovery_frames) < 256:
                    self.discovery_frames.add(frame)
                elif method == 'Page.frameDetached':
                    self.discovery_frames.discard(frame)
        if method == 'Target.attachedToTarget' and params.get('waitingForDebugger') is True:
            kind = params.get('targetInfo', {}).get('type')
            if kind not in ('page', 'iframe', 'worker', 'service_worker', 'shared_worker'):
                raise ReplayError('unsupported_replay_target')
            self.bind(params.get('sessionId'), kind, message, True,params.get('targetInfo',{}).get('targetId'),message.get('sessionId'))
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
            result, reason = self.archive.response(request, params.get('resourceType'))
            if result is None:
                if self.discovery:
                    frame = params.get('frameId')
                    child_document = self.discovery_session is not None and params.get('resourceType') == 'Document' and (
                        session == self.discovery_session
                        and isinstance(frame, str) and frame in self.discovery_frames
                        or self.sessions[session] == 'iframe'
                        and self.session_parents.get(session) == self.discovery_session)
                    owned_scope = self.discovery_session is not None and (session == self.discovery_session
                        or self.sessions[session] == 'iframe' and self.session_parents.get(session) == self.discovery_session)
                    asset_kind = {'Script':'script','Stylesheet':'stylesheet','Font':'font','Image':'image',
                                  'Fetch':'json','XHR':'json','Manifest':'manifest'}.get(params.get('resourceType')) if owned_scope else None
                    if owned_scope and params.get('resourceType') == 'Other' and request.get('url') in self.archive.fetch_preloads:
                        asset_kind = 'json'
                    self.propose(request, reason, child_document, asset_kind)
                self.call('Fetch.failRequest', {'requestId': paused_id, 'errorReason': 'BlockedByClient'}, session, ('reply', session, 'blocked', reason, False))
            else:
                self.call('Fetch.fulfillRequest', {'requestId': paused_id, **result}, session, ('reply', session, 'served', request.get('method') not in ('GET', 'HEAD'), False))
            return False
        return True
