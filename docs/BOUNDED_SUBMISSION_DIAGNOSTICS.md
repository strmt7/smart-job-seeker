# Bounded fictional submission diagnostics

Checkpoint: 2026-11-10. The `f77f70c5` hosted browser run passed 136 cases and
failed one: `generic_click_can_trigger_get_submission_on_loopback`. Initial
navigation returned `net::ERR_CONNECTION_ABORTED`, before the diagnostic click.
The hosted log does not identify the precise transport failure. Workspace tests,
the compiled build/layout job, dependency checks, coverage and CodeQL passed
for that commit; the browser job did not.

The GET and POST scripted-submission diagnostics each still used an independent
TCP server that read once, handled clients serially, swallowed read/write errors
and expired 15 seconds after creation, including browser startup. A TCP read
does not establish a complete HTTP request. Neither diagnostic had adopted the
existing shared fixture's concurrent speculative-connection handling and bounded
request/body framing.

Both now reuse that fixture, with response-body observation added alongside the
existing 204 channel-observer mode. It drains complete bounded requests, handles
idle connections concurrently and joins owned threads even if navigation panics.
Errors remain test failures. There is no browser-navigation retry, ignored
failure or relaxed assertion. Both diagnostics still require observing the
actual fictional submission request; neither is a passing submission-prevention
test. The production browser and real-site restrictions are unchanged.

Eight ordinary fixture checks passed, including a new regression that fragments
the initial GET while an idle connection is open, verifies no early observation
and then verifies the complete page response. Both affected headless native
diagnostics passed against the explicitly selected local Chrome executable.
Scoped browser-crate Clippy, formatting and diff checks passed. Private logs are
`target/diagnostic-fixture-tests.log`, `target/diagnostic-browser-tests.log` and
`target/diagnostic-fixture-clippy.log`. These are focused local results, not a
new full native-suite result or proof of the original runner's exact failure
cause. Follow-up hosted results must be checked against their own commit.

The known gap remains: an ordinary control can trigger scripted POST or GET
submission. Only fictional loopback forms were interacted with. No real
application, application email or release was created.
