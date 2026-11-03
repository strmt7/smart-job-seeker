import unittest

from inert_form_fixture import make_fixture


class FixtureTests(unittest.TestCase):
    def test_removes_active_content_urls_and_existing_values(self):
        source = '''<form action="https://example.invalid/submit" method="post">
        <script>fetch('https://example.invalid/write')</script>
        <label for="x">Full name</label><input id="x" type="text" value="PRIVATE" oninput="submit()" required>
        <label>Country<select><option value="SECRET">France</option></select></label>
        <textarea>PRIVATE TEXT</textarea><iframe src="https://example.invalid/"></iframe>
        <button type="submit">Apply</button></form>'''
        result = make_fixture(source)
        for forbidden in ("<form", "<script", "https://", "PRIVATE", "SECRET", "oninput", "<iframe", "<button"):
            self.assertNotIn(forbidden, result)
        self.assertIn('Full name</label><input id="field-0" type="text" required>', result)
        self.assertIn('Country</label><select id="field-1"><option>France</option>', result)
        self.assertIn('<textarea id="field-2"></textarea>', result)

    def test_escapes_untrusted_text_and_omits_submit_controls(self):
        result = make_fixture('<label for="a">&lt;img src=x onerror=alert(1)&gt;</label><input id="a"><input type="submit">')
        self.assertIn('&lt;img src=x onerror=alert(1)&gt;', result)
        self.assertNotIn('<img', result)
        self.assertEqual(result.count('<input'), 1)

    def test_caps_number_of_controls(self):
        result = make_fixture('<input>' * 501)
        self.assertEqual(result.count('<input'), 500)


if __name__ == "__main__":
    unittest.main()
