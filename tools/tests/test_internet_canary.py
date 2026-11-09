"""Hermetic checks; no public request or account is used by this test module."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('internet_canary_tests', Path(__file__).resolve().parents[1] / 'internet_canary.py')
canary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(canary)
TOKEN = '11111111-2222-4333-8444-555555555555'
NONCE = '123456781234423482341234567890ab'


def row(stage, method='POST'):
    return {'token_id': TOKEN, 'method': method, 'query': {'stage': stage, 'probe': NONCE},
            'content': f'fictional-qualification:{stage}:{NONCE}'}


class InternetCanaryTests(unittest.TestCase):
    def test_receiver_counts_all_arrivals_and_only_exact_post_markers(self):
        rows = [row('control_before'), row('isolated', 'OPTIONS'), row('control_after')]
        result = canary.request_summary({'total': 3, 'data': rows}, TOKEN, NONCE)
        self.assertEqual(result['posts']['isolated'], 0)
        self.assertEqual(result['arrivals']['isolated'], 1)
        self.assertEqual(result['posts']['control_before'], 1)
        self.assertNotIn('query', str(result))

    def test_unknown_or_truncated_receiver_data_is_never_zero_traffic(self):
        bad = [None, {'total': True, 'data': []}, {'total': 2, 'data': []},
               {'total': 12, 'data': [row('control_before')] * 12}]
        for key, value in [('token_id', 'other'), ('query', {}), ('content', 'unapproved marker')]:
            bad.append({'total': 1, 'data': [dict(row('isolated'), **{key: value})]})
        for data in bad:
            with self.subTest(data=data), self.assertRaises(canary.CanaryUnavailable):
                canary.request_summary(data, TOKEN, NONCE)

    def test_failed_controls_and_successful_isolated_browser_are_refused(self):
        receiver = canary.InternetCanary(TOKEN, NONCE)
        good = [{'stage': stage, 'result': 'failed' if stage == 'isolated' else 'delivered'} for stage in canary.PHASES]
        with self.assertRaises(canary.CanaryUnavailable): receiver.verify(good[:2])
        wrong = [dict(item) for item in good]; wrong[0]['result'] = 'failed'
        with self.assertRaises(canary.CanaryUnavailable): receiver.verify(wrong)
        wrong = [dict(item) for item in good]; wrong[1]['result'] = 'delivered'
        with self.assertRaises(canary.CanaryLeak): receiver.verify(wrong)

    def test_receiver_detects_arrival_even_when_browser_reports_failure(self):
        receiver = canary.InternetCanary(TOKEN, NONCE)
        states = [{'stage': stage, 'result': 'failed' if stage == 'isolated' else 'delivered'} for stage in canary.PHASES]
        data = {'total': 3, 'data': [row(stage) for stage in canary.PHASES]}
        with patch.object(receiver, 'snapshot', return_value=canary.request_summary(data, TOKEN, NONCE)):
            with self.assertRaises(canary.CanaryLeak): receiver.verify(states)

    def test_healthy_controls_require_a_complete_observation_window(self):
        receiver = canary.InternetCanary(TOKEN, NONCE)
        states = [{'stage': stage, 'result': 'failed' if stage == 'isolated' else 'delivered'} for stage in canary.PHASES]
        data = {'total': 2, 'data': [row('control_before'), row('control_after')]}
        ticks = iter(range(100))
        with patch.object(receiver, 'snapshot', return_value=canary.request_summary(data, TOKEN, NONCE)), \
                patch.object(canary.time, 'monotonic', side_effect=lambda: next(ticks)), \
                patch.object(canary.time, 'sleep'):
            result = receiver.verify(states)
        self.assertTrue(result['protected_in_observed_case'])
        self.assertGreaterEqual(result['receiver_observations'], 2)
        self.assertEqual(result['healthy_observation_seconds'], 10)

    def test_free_creation_requests_expiry_no_actions_and_no_account_key(self):
        with patch.object(canary, 'api', return_value=(201, {'uuid': TOKEN, 'expires_at': 'fixture expiry'})) as api:
            receiver = canary.InternetCanary.create()
        args = api.call_args.args
        self.assertEqual(args[:2], ('POST', '/token'))
        self.assertEqual(args[2]['expiry'], 900)
        self.assertFalse(args[2]['actions'])
        self.assertNotIn('api_key', args[2])
        with patch.object(canary, 'api', return_value=(204, None)) as api:
            receiver.close(); receiver.close()
        api.assert_called_once_with('DELETE', '/token/' + TOKEN)


if __name__ == '__main__':
    unittest.main()
