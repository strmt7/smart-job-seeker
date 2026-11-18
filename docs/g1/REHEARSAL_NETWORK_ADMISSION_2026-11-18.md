# Network checks before saved application rehearsal

The Windows worker reads the loopback exemption table for its exact AppContainer
SID before starting the browser. An exemption or failed policy query stops
admission. The browser runs with zero capabilities in an owned Windows job.
[Windows loopback configuration](https://learn.microsoft.com/en-us/windows/win32/api/netfw/nf-netfw-networkisolationgetappcontainerconfig)

Before it copies the archived page or candidate PDFs, the worker opens a
disposable browser context. It checks its live loopback listener with a
positive connection, then attempts a direct connection from the contained
browser. Readiness requires the observed network error and no connection at
the listener. It repeats on IPv6 when the local address family is available.
The context is disposed and its absence verified before files are mounted.
The worker checks the Windows exemption table again. Replay reports retain
the resulting observations. [Browser context lifecycle](https://chromedevtools.github.io/devtools-protocol/tot/Target/#method-disposeBrowserContext)

## Verification

The non-elevated native preflight read the existing Windows policy and
observed `ERR_CONNECTION_TIMED_OUT` for both address families. A trusted local
connection reached each listener first. The isolated browser reached neither.
An early version then stalled during target inventory after closing the probe
tab. The corrected version disposes the entire probe context and checks the
context inventory. The original failure and focused reruns are retained in
`target/rehearsal-qualification-lrfkqppi` and
`target/appcontainer-network-ebe4df8b`. Browser errors outside the admitted
observations still require review.

The native matrix on HeadlessChrome 157.0.8103.38 passed four browser cases,
one recovery case and seven Windows worker cases. The scripted IPv4 and IPv6
channels and WebRTC relay tests used matching unrestricted controls with live
receivers. The additional readiness case deliberately contacted the local
canary from the test controller. The worker refused admission before staging
content or attachments and removed its profile. See
`target/rehearsal-qualification-d8j8pfrf`.

Two native replay cases passed, covering normal replay and target alias
cleanup. The browser reported eight recorded responses, one blocked request
and one cached non-read reply. The latter is a recorded response delivered
inside replay. The compiled app preserved the two loopback timeout observations
in a saved report and opened one read-only view after refusing an invalid
model selection. See `target/replay-qualification-cwqctsww` and
`target/appcontainer-network-ebe4df8b/archived-report`.

The Windows workspace had 1,479 passing tests and 622 ignored cases. Clippy,
the canonical Windows build and 89 Python worker/replay tests passed.

The first browser protocol documentation capture returned the viewer home
page before its navigation completed. The settled capture contains the target
domain descriptions. Native documentation receipts remain under
`target/appcontainer-network-ebe4df8b`.

The network evidence here is for the measured Windows profile, browser build
and test transports. Live employer actions and server-side behavior still
require separate evidence and authorization. No test used candidate data or a
real employer endpoint.
