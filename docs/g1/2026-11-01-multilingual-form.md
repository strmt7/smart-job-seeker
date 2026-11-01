# Local multilingual form check — 2026-11-01

On the development Windows host, the resident `qwen3.5:9b` model identified
first-name and email fields on a fictional German-language form. Waypoint then
filled those two fields in a local browser, checked their current values, and
asked the same model to inspect a screenshot. Both values matched the visual
transcriptions, while five other fields remained empty. No form was sent.

The extended `live_multilingual_browser_form_uses_local_model_without_sensitive_fields`
test passed twice. It used a 4,096-token context and synthetic contact details.
The test covers one simple static fixture; it does not establish reliability
on other layouts, languages, multi-step applications, or real employer sites.
