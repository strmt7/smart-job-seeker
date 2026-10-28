// Adapted from rejection-rejector commit 48c672b29cf098ff5da10d324de3e87766a2f483. See docs/REJECTION_REJECTOR_LICENSE.md.
use crate::gmail_net as net;
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::Write,
    net::TcpListener,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

mod callback;

/// Google OAuth2 token endpoint.
///
/// Inputs: none (constant). Output: `&str` — the endpoint used to exchange
/// and refresh tokens.
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// OAuth scope granting Gmail read-only access.
///
/// Inputs: none (constant). Output: `&str` — the scope requested at consent.
pub const READ_SCOPE: &str = "https://www.googleapis.com/auth/gmail.readonly";
/// OAuth scope granting Gmail send access.
///
/// Inputs: none (constant). Output: `&str` — the scope requested for
/// controlled sending.
pub const SEND_SCOPE: &str = "https://www.googleapis.com/auth/gmail.send";

/// Long-lived OAuth client and refresh credentials.
///
/// Inputs: none (plain struct). Output: the client id, client secret,
/// refresh token and whether the send scope was granted.
// Credentials intentionally do not implement Debug to prevent accidental logging of secrets.
// The struct implements Drop to zeroize sensitive fields on destruction.
// Handle the return value of oauth::login() carefully as it contains sensitive credential material.
#[derive(Clone, Serialize, Deserialize)]
pub struct Credentials {
    /// OAuth client identifier.
    pub client_id: String,
    /// OAuth client secret.
    pub client_secret: String,
    /// OAuth refresh token.
    pub refresh_token: String,
    /// Whether the granted scopes permit sending.
    pub can_send: bool,
}
impl Drop for Credentials {
    /// Zeroize the stored client credentials on drop.
    ///
    /// Inputs: none (`&mut self`). Output: none — the client secret and refresh
    /// token are wiped.
    fn drop(&mut self) {
        self.client_secret.zeroize();
        self.refresh_token.zeroize();
    }
}
#[derive(Deserialize)]
struct ClientFile {
    installed: Installed,
}
#[derive(Deserialize)]
struct Installed {
    client_id: String,
    client_secret: String,
}
impl Drop for Installed {
    /// Zeroize the installed-app client secret on drop.
    ///
    /// Inputs: none (`&mut self`). Output: none — the client secret is wiped.
    fn drop(&mut self) {
        self.client_secret.zeroize();
    }
}
/// OAuth token endpoint response.
///
/// Inputs: none (plain struct). Output: the access token, expiry, optional
/// rotated refresh token and the granted scope.
#[derive(Deserialize)]
pub struct Tokens {
    /// OAuth access token.
    pub access_token: String,
    /// Access-token lifetime in seconds.
    pub expires_in: u64,
    /// OAuth refresh token.
    pub refresh_token: Option<String>,
    /// Space-separated granted OAuth scopes.
    pub scope: Option<String>,
}
impl Zeroize for Tokens {
    /// Wipe the stored tokens in place.
    ///
    /// Inputs: none (`&mut self`). Output: none — the access token and any refresh
    /// token are zeroized.
    fn zeroize(&mut self) {
        self.access_token.zeroize();
        if let Some(token) = &mut self.refresh_token {
            token.zeroize();
        }
    }
}
/// Generate a fresh URL-safe random secret.
///
/// Inputs: none. Output: `String` — the URL-safe base64 encoding of 32 random
/// bytes.
pub fn secret() -> Result<String> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|_| anyhow::anyhow!("Random generation failed"))?;
    Ok(URL_SAFE_NO_PAD.encode(b))
}

/// RFC 7636 S256 code challenge: BASE64URL-ENCODE(SHA256(ASCII(verifier))).
///
/// Inputs: `verifier` — PKCE code verifier. Output: `String` — the base64url
/// (no padding) S256 code challenge.
///
/// Split out of `login()` so the exact challenge construction is pinned by the
/// RFC test vector; a subtly wrong digest encoding would break authorization
/// only at the Google redirect, far from any test.
fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Extract the authorization code from one loopback callback target.
///
/// Inputs: `target` — request target (path and query); `state` — the expected
/// OAuth state. Output: `Result<Option<String>>` — `Some(code)` for a matching
/// OAuth callback, `None` for other valid targets; error when the target is
/// malformed (fail closed).
fn callback_code(target: &str, state: &str) -> Result<Option<String>> {
    ensure!(
        target.starts_with('/')
            && !target.starts_with("//")
            && !target.contains('#')
            && !target.chars().any(char::is_control),
        "Invalid OAuth callback target"
    );
    let u = url::Url::parse(&format!("http://127.0.0.1{target}"))?;
    if u.path() != "/callback" {
        return Ok(None);
    }
    let pairs: Vec<_> = u.query_pairs().collect();
    ensure!(
        pairs.iter().filter(|(k, _)| k == "state").count() == 1,
        "Invalid OAuth state count"
    );
    ensure!(
        pairs.iter().filter(|(k, _)| k == "code").count() == 1,
        "Exactly one OAuth authorization code is required"
    );
    let q: HashMap<_, _> = pairs.into_iter().collect();
    let actual = q.get("state").map_or("", |v| v.as_ref());
    ensure!(
        actual.as_bytes().ct_eq(state.as_bytes()).unwrap_u8() == 1,
        "Invalid OAuth state"
    );
    ensure!(
        !q.contains_key("error"),
        "Google authorization was declined"
    );
    let code = q.get("code").context("Authorization code missing")?;
    ensure!(
        !code.is_empty() && code.len() < 4096,
        "Invalid authorization code"
    );
    Ok(Some(code.to_string()))
}

/// Desktop-app OAuth: S256 PKCE, random state, random loopback port, five-minute expiry.
///
/// Inputs: `path` — Google Desktop-app OAuth JSON; `send` — whether the send
/// scope is required; `cancelled` — cooperative cancellation flag. Output:
/// the obtained [`Credentials`]. Opens the user's system browser with
/// [`webbrowser::open`]; the launcher failure mapping lives in the private `login_with`
/// so it is exercised without launching a real browser.
pub fn login(client_json: Vec<u8>, send: bool, cancelled: &AtomicBool) -> Result<Credentials> {
    login_with(
        client_json,
        send,
        cancelled,
        Duration::from_secs(300),
        webbrowser::open,
        TOKEN_URL,
    )
}

/// Parameterized core of [`login`].
///
/// Inputs: `path` — Google Desktop-app OAuth JSON; `send` — whether the send
/// scope is required; `cancelled` — cooperative cancellation flag;
/// `timeout` — consent deadline; `open_browser` — consent-URL launcher;
/// `token_url` — OAuth token endpoint. Output: the obtained
/// [`Credentials`]. The browser launcher and token endpoint are injectable so
/// the whole consent loop (state, PKCE binding, decline handling, exchange
/// failure modes) can be exercised deterministically without a real browser
/// or Google; [`login`] wires in the real ones. A launcher failure of any
/// error type is mapped to "Cannot open the system browser for Google
/// sign-in".
fn login_with<E: Into<anyhow::Error>>(
    client_json: Vec<u8>,
    send: bool,
    cancelled: &AtomicBool,
    timeout: Duration,
    open_browser: impl Fn(&str) -> std::result::Result<(), E>,
    token_url: &str,
) -> Result<Credentials> {
    ensure!(client_json.len() <= 32768, "OAuth client file is too large");
    let config_bytes = Zeroizing::new(client_json);
    let mut config: ClientFile = serde_json::from_slice(&config_bytes).map_err(|_| {
        anyhow::anyhow!(
            "Choose a Google Desktop app OAuth JSON file containing an installed object"
        )
    })?;
    drop(config_bytes);
    ensure!(
        !config.installed.client_id.is_empty() && !config.installed.client_secret.is_empty(),
        "Incomplete OAuth client file"
    );
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let redirect = format!(
        "http://127.0.0.1:{}/callback",
        listener.local_addr()?.port()
    );
    let state = secret()?;
    let verifier = Zeroizing::new(secret()?);
    let challenge = pkce_challenge(verifier.as_str());
    let scope = if send {
        format!("{READ_SCOPE} {SEND_SCOPE}")
    } else {
        READ_SCOPE.into()
    };
    let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", config.installed.client_id.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("response_type", "code"),
        ("scope", scope.as_str()),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("state", state.as_str()),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
    ]);
    open_browser(url.as_str()).map_err(|error| {
        let error: anyhow::Error = error.into();
        anyhow::anyhow!("Cannot open the system browser for Google sign-in: {error}")
    })?;
    let login_deadline = Instant::now() + timeout;
    let local_port = listener.local_addr()?.port();
    let code = consent_code(
        TcpListener::accept,
        &listener,
        local_port,
        state.as_str(),
        cancelled,
        login_deadline,
    )?;
    // Do not exchange an authorization code after cancellation or expiry.
    ensure!(!cancelled.load(Ordering::SeqCst), "Authorization cancelled");
    ensure!(Instant::now() < login_deadline, "Authorization expired");
    drop(listener);
    let response = net::client(30, false)?
        .post(token_url)
        .form(&[
            ("client_id", config.installed.client_id.as_str()),
            ("client_secret", config.installed.client_secret.as_str()),
            ("code", code.as_str()),
            ("code_verifier", verifier.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .context("OAuth exchange failed")?;
    let mut tokens: Zeroizing<Tokens> = Zeroizing::new(net::json(response, 32768)?);
    ensure!(tokens.expires_in > 0, "Invalid token lifetime");
    let granted = tokens.scope.as_deref().unwrap_or("");
    ensure!(
        granted.split_whitespace().any(|s| s == READ_SCOPE),
        "Gmail read permission was not granted"
    );
    let can_send = granted.split_whitespace().any(|s| s == SEND_SCOPE);
    ensure!(!send || can_send, "Gmail send permission was not granted");
    let Some(refresh_token) = tokens.refresh_token.take() else {
        tokens.access_token.zeroize();
        bail!(
            "No refresh token returned. Revoke this app's old consent in your Google account and reconnect"
        );
    };
    tokens.access_token.zeroize();
    Ok(Credentials {
        client_id: std::mem::take(&mut config.installed.client_id),
        client_secret: std::mem::take(&mut config.installed.client_secret),
        refresh_token,
        can_send,
    })
}

/// One accept step on the loopback consent listener.
type AcceptFn = fn(&TcpListener) -> std::io::Result<(std::net::TcpStream, std::net::SocketAddr)>;

/// Drive the loopback consent callback until an authorization code arrives.
///
/// Inputs: `accept` — one accept step on the listener (production passes
/// [`TcpListener::accept`]; tests inject an always-failing step to pin the
/// error mapping without a real socket failure); `listener` — loopback
/// listener bound to `127.0.0.1`, so every accepted peer is loopback by
/// construction and no peer filter is needed; `local_port` — port advertised
/// in the redirect URI; `state` — expected OAuth state; `cancelled` —
/// cooperative cancellation flag; `login_deadline` — consent deadline.
/// Output: the authorization code, or an error for cancellation, expiry, a
/// listener or socket failure, or an unreadable callback request.
fn consent_code(
    accept: AcceptFn,
    listener: &TcpListener,
    local_port: u16,
    state: &str,
    cancelled: &AtomicBool,
    login_deadline: Instant,
) -> Result<Zeroizing<String>> {
    let code = loop {
        ensure!(!cancelled.load(Ordering::SeqCst), "Authorization cancelled");
        ensure!(
            Instant::now() < login_deadline,
            "Authorization timed out; no credentials saved"
        );
        match accept(listener) {
            Ok((mut stream, _peer)) => {
                stream.set_read_timeout(Some(Duration::from_millis(250)))?;
                stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                let deadline = (Instant::now() + Duration::from_secs(2)).min(login_deadline);
                let result =
                    callback::read_headers(&mut stream, cancelled, deadline).and_then(|headers| {
                        let target = callback::request_target(&headers, local_port)?;
                        callback_code(target, state)
                    });
                let (status, text) = if matches!(&result, Ok(Some(_))) {
                    ("200 OK", "Authorization received. Return to Waypoint.")
                } else {
                    (
                        "400 Bad Request",
                        "Authorization not accepted. Retry from the application.",
                    )
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
                let _ = stream.write_all(response.as_bytes());
                if let Ok(Some(code)) = result {
                    break code;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(e) => return Err(e.into()),
        }
    };
    Ok(Zeroizing::new(code))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_requires_unique_matching_state_and_code() {
        assert_eq!(
            callback_code("/callback?code=x&state=correct", "correct").unwrap(),
            Some("x".into())
        );
        assert!(callback_code("/callback?code=x&state=wrong", "correct").is_err());
        assert!(callback_code("/callback?code=x&state=correct&state=correct", "correct").is_err());
        assert!(callback_code("//example.org/callback?code=x&state=correct", "correct").is_err());
    }
    #[test]
    fn pkce_matches_rfc_vector() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
