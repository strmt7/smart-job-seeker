import asyncio
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import AsyncMock, patch

from tools import crawl4ai_research as research


class CaptureTests(unittest.TestCase):
    def test_ordinary_launch_removes_upstream_automation_mask(self):
        masking = '--disable-blink-features=AutomationControlled'
        options = {'headless': True, 'args': ['--disable-gpu', masking]}
        filtered = research.ordinary_launch_args(options)
        self.assertEqual(filtered['args'], ['--disable-gpu'])
        self.assertEqual(options['args'], ['--disable-gpu', masking])
        with self.assertRaisesRegex(ValueError, 'review the provider'):
            research.ordinary_launch_args({'args': ['--disable-gpu']})

    def test_crawler_uses_identified_nonstealth_configuration(self):
        class Config:
            def __init__(self, **kwargs):
                self.options = kwargs
                self.headers = {'sec-ch-ua': 'fictional old browser'}

        class Crawler:
            def __init__(self, config):
                self.config = config
                manager = SimpleNamespace(_build_browser_args=lambda: {
                    'args': ['--disable-blink-features=AutomationControlled']
                })
                self.crawler_strategy = SimpleNamespace(browser_manager=manager)

        module = SimpleNamespace(AsyncWebCrawler=Crawler, BrowserConfig=Config)
        with patch.dict(sys.modules, {'crawl4ai': module}), \
                patch.object(research.importlib.metadata, 'version', return_value=research.VERSION):
            instance = research.crawler()
        self.assertFalse(instance.config.options['enable_stealth'])
        self.assertEqual(instance.config.user_agent, 'WaypointResearch/1.0')
        self.assertNotIn('sec-ch-ua', instance.config.headers)
        self.assertEqual(instance.crawler_strategy.browser_manager._build_browser_args()['args'], [])

    def test_native_result_and_robots_configuration_are_retained(self):
        text = 'Fictional public page. ' * 15
        result = SimpleNamespace(
            markdown=SimpleNamespace(raw_markdown=text), status_code=200,
            redirected_status_code=None, response_headers={}, error_message='', success=True,
            model_dump=lambda **_: {'url': 'https://example.org/', 'markdown': text},
        )
        owner = SimpleNamespace(arun=AsyncMock(return_value=result))
        module = SimpleNamespace(CacheMode=SimpleNamespace(BYPASS='bypass'),
                                 CrawlerRunConfig=lambda **kwargs: kwargs)
        with tempfile.TemporaryDirectory() as temporary, \
                patch.dict(sys.modules, {'crawl4ai': module}), \
                patch.object(research.importlib.metadata, 'version', return_value='fixture'):
            path = Path(temporary) / 'capture.json'
            row = asyncio.run(research.fetch(owner, 'https://example.org/', 'public page', path))
            self.assertTrue(row['success'])
            self.assertTrue(owner.arun.call_args.kwargs['config']['check_robots_txt'])
            self.assertEqual(owner.arun.call_args.kwargs['config']['page_timeout'], 30000)
            self.assertEqual(owner.arun.call_args.kwargs['config']['delay_before_return_html'], 0.1)
            capture = json.loads(path.read_text(encoding='utf-8'))
            self.assertEqual(capture['provider_version'], 'fixture')
            self.assertEqual(capture['native_result']['markdown'], text)
            with self.assertRaises(FileExistsError):
                research.retain_capture(path, 'https://example.org/', result, row)

    def test_provider_failure_is_written_and_private_url_is_refused(self):
        with tempfile.TemporaryDirectory() as temporary, \
                patch.object(research, 'crawler', side_effect=RuntimeError('fixture failure')), \
                patch.object(research.importlib.metadata, 'version', return_value='fixture'):
            path = Path(temporary) / 'failure.json'
            args = SimpleNamespace(command='capture', url='https://example.org/', marker='', capture=path, settle_seconds=0.1)
            self.assertEqual(asyncio.run(research.execute(args)), 1)
            self.assertEqual(json.loads(path.read_text(encoding='utf-8'))['diagnosis']['failure_category'], 'provider_error')
            args.url = 'https://127.0.0.1/private'
            with self.assertRaisesRegex(ValueError, 'nonpublic'):
                asyncio.run(research.execute(args))
            args.url = 'https://example.org/'
            args.settle_seconds = 11
            with self.assertRaisesRegex(ValueError, 'limit'):
                asyncio.run(research.execute(args))


if __name__ == '__main__':
    unittest.main()
