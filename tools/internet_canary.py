"""Opt-in development-only Internet receiver; never an app runtime service.

Uses the public free API without an account/key. Only fictional markers are
allowed. Tokens expire after 15 minutes and are deleted after qualification.
Request IPs, headers and bodies from the service are never persisted here.
"""
from __future__ import annotations
import json
from pathlib import Path
import time
import urllib.error
import urllib.request
import uuid

ORIGIN = 'https://webhook.site'
PHASES = ('control_before', 'isolated', 'control_after')
REQUEST_LIMIT = 12


class CanaryUnavailable(RuntimeError):
    pass


class CanaryLeak(RuntimeError):
    pass


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def api(method, path, payload=None):
    if method not in ('GET', 'POST', 'DELETE') or not path.startswith('/token') or '\r' in path or '\n' in path:
        raise ValueError('Invalid canary API operation')
    body = None if payload is None else json.dumps(payload).encode('utf-8')
    request = urllib.request.Request(ORIGIN + path, data=body, method=method, headers={
        'Accept': 'application/json', 'Content-Type': 'application/json',
        'Cache-Control': 'no-cache', 'User-Agent': 'SmartJobseeker-Qualification/1.0'})
    try:
        with urllib.request.build_opener(NoRedirect).open(request, timeout=10) as response:
            raw = response.read(256 * 1024 + 1)
            if len(raw) > 256 * 1024:
                raise CanaryUnavailable('Canary API response exceeds its budget')
            return response.status, json.loads(raw) if raw else None
    except urllib.error.HTTPError as error:
        if method == 'DELETE' and error.code == 404:
            return 404, None
        raise CanaryUnavailable(f'Canary API {method} returned HTTP {error.code}') from None
    except (OSError, ValueError) as error:
        raise CanaryUnavailable('Canary API transport or response failed: ' + type(error).__name__) from None


def request_summary(payload, token, nonce):
    if not isinstance(payload, dict) or type(payload.get('total')) is not int:
        raise CanaryUnavailable('Missing receiver denominator')
    rows, total = payload.get('data'), payload['total']
    if not isinstance(rows, list) or not 0 <= total < REQUEST_LIMIT or len(rows) != total:
        raise CanaryUnavailable('Receiver history is incomplete or at its retention limit')
    posts, arrivals = ({phase: 0 for phase in PHASES} for _ in range(2))
    for row in rows:
        if not isinstance(row, dict) or row.get('token_id') != token:
            raise CanaryUnavailable('Wrong receiver collection')
        query = row.get('query')
        if not isinstance(query, dict):
            raise CanaryUnavailable('Missing receiver query markers')
        def one(key):
            value = query.get(key)
            return value[0] if isinstance(value, list) and len(value) == 1 else value
        phase = one('stage')
        if phase not in PHASES or one('probe') != nonce:
            raise CanaryUnavailable('Unexpected receiver marker')
        arrivals[phase] += 1
        if row.get('method') == 'POST':
            if row.get('content') != f'fictional-qualification:{phase}:{nonce}':
                raise CanaryUnavailable('Unexpected receiver body')
            posts[phase] += 1
    return {'total': total, 'posts': posts, 'arrivals': arrivals}


class InternetCanary:
    def __init__(self, token, nonce):
        if str(uuid.UUID(token)) != token or uuid.UUID(nonce).hex != nonce:
            raise ValueError('Invalid ephemeral canary identifiers')
        self.token, self.nonce, self.closed = token, nonce, False
        self.reported_expiry = None

    @classmethod
    def create(cls):
        code, data = api('POST', '/token', {'expiry': 900, 'request_limit': REQUEST_LIMIT,
            'cors': True, 'actions': False, 'default_status': 204, 'default_content': ''})
        if code not in (200, 201) or not isinstance(data, dict):
            raise CanaryUnavailable('Free receiver creation did not succeed')
        try:
            result = cls(data['uuid'], uuid.uuid4().hex)
        except (KeyError, ValueError, TypeError):
            raise CanaryUnavailable('Free receiver returned an invalid identity') from None
        result.reported_expiry = data.get('expires_at')
        return result

    def write_spec(self, path: Path):
        path.write_text(json.dumps({'schema': 1, 'root_url': ORIGIN + '/' + self.token,
                                   'nonce': self.nonce}), encoding='utf-8')

    def snapshot(self):
        status, data = api('GET', f'/token/{self.token}/requests?per_page=20&sorting=oldest')
        if status != 200:
            raise CanaryUnavailable('Receiver inspection failed')
        return request_summary(data, self.token, self.nonce)

    def verify(self, browser_observations):
        if (not isinstance(browser_observations, list) or len(browser_observations) != 3
                or [row.get('stage') for row in browser_observations] != list(PHASES)):
            raise CanaryUnavailable('Missing browser observation denominator')
        states = {row['stage']: row.get('result') for row in browser_observations}
        if states['control_before'] != 'delivered' or states['control_after'] != 'delivered':
            raise CanaryUnavailable('Public browser controls did not complete')
        if states['isolated'] != 'failed':
            raise CanaryLeak('Isolated public request did not report failure')
        deadline, healthy_at, observations = time.monotonic() + 35, None, 0
        while time.monotonic() < deadline:
            summary = self.snapshot()
            observations += 1
            if summary['arrivals']['isolated']:
                raise CanaryLeak('Isolated traffic reached the public receiver')
            if summary['posts']['control_before'] and summary['posts']['control_after']:
                healthy_at = time.monotonic() if healthy_at is None else healthy_at
                if time.monotonic() - healthy_at >= 10:
                    return {'protected_in_observed_case': True, 'receiver': summary,
                            'receiver_observations': observations, 'healthy_observation_seconds': 10,
                            'scope': 'one public HTTPS receiver; fictional POST; not universal egress qualification'}
            time.sleep(2)
        raise CanaryUnavailable('Public receiver controls or observation window were incomplete')

    def close(self):
        if not self.closed:
            code, _ = api('DELETE', '/token/' + self.token)
            if code not in (204, 404):
                raise CanaryUnavailable('Temporary receiver deletion was not confirmed')
            self.closed = True
