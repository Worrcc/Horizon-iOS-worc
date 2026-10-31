//! Microsoft account sign-in in the game itself, for systems with no launcher to do it (the iOS
//! app): the device-code flow, Xbox Live, XSTS and the Minecraft services, as the launcher's
//! `account.rs` does it (docs/microsoft-account.md describes each step). The result is an
//! [`auth::AuthSession`](crate::auth::AuthSession), the same the launcher hands over in a session
//! file, and a [`Stored`] account for the system's keychain. Tokens are never logged.
use serde_json::{Value, json};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SCOPE: &str = "XboxLive.signin offline_access";
/// A Minecraft token is renewed when it has less than this left.
const RENEW_BEFORE: u64 = 3600;

pub const NOT_CONFIGURED: &str = "This build has no Microsoft sign-in: it was built without an \
     Azure app (client ID). The Horizon-iOS repository's ios/README.md, \"Microsoft sign-in\", \
     explains how to set one up.";
const DOES_NOT_OWN: &str = "This account doesn't own Minecraft: Java Edition.";
const NO_PROFILE: &str = "This account has no Minecraft profile yet: play once with the official \
     launcher to create one.";
const SIGN_IN_AGAIN: &str = "Your Microsoft sign-in expired. Sign in again.";

/// The Azure app: the run-time variable wins over the build-time one; empty means none.
pub fn client_id() -> Option<String> {
    std::env::var("HORIZON_MSA_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("HORIZON_MSA_CLIENT_ID").map(str::to_owned))
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
}

const MICROSOFT: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
const XBOX_USER: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MINECRAFT: &str = "https://api.minecraftservices.com";

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn request(method: &str, url: &str) -> ureq::Request {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .user_agent(&format!("horizon/{}", crate::crash::version()))
        .build()
        .request(method, url)
}

enum Body<'a> {
    Form(Vec<(&'a str, &'a str)>),
    Json(Value),
}

/// The status and JSON body (null when there is none). Transport errors name the service only.
fn send(request: ureq::Request, body: Option<Body>, service: &str) -> Result<(u16, Value), String> {
    let result = match body {
        None => request.call(),
        Some(Body::Form(fields)) => request.send_form(&fields),
        Some(Body::Json(value)) => request
            .set("Accept", "application/json")
            .set("Content-Type", "application/json")
            .send_string(&value.to_string()),
    };
    let response = match result {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(ureq::Error::Transport(_)) => {
            return Err(format!(
                "Could not reach {service}. Check your internet connection."
            ));
        }
    };
    let status = response.status();
    let text = response.into_string().unwrap_or_default();
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// What the player types at the verification page.
#[derive(Clone)]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    device_code: String,
    interval: u64,
    expires_at: u64,
}

/// Starts a sign-in: the code to show the player.
pub fn device_code(client_id: &str) -> Result<DeviceCode, String> {
    let (status, v) = send(
        request("POST", &format!("{MICROSOFT}/devicecode")),
        Some(Body::Form(vec![("client_id", client_id), ("scope", SCOPE)])),
        "Microsoft",
    )?;
    match (
        status,
        str_field(&v, "user_code"),
        str_field(&v, "device_code"),
    ) {
        (200, Some(user_code), Some(device_code)) => Ok(DeviceCode {
            user_code,
            verification_uri: str_field(&v, "verification_uri")
                .unwrap_or_else(|| "https://www.microsoft.com/link".into()),
            interval: v
                .get("interval")
                .and_then(Value::as_u64)
                .unwrap_or(5)
                .max(1),
            expires_at: now() + v.get("expires_in").and_then(Value::as_u64).unwrap_or(900),
            device_code,
        }),
        _ => Err(oauth_error(&v)),
    }
}

/// The OAuth error codes as messages for the player.
fn oauth_error(v: &Value) -> String {
    match v.get("error").and_then(Value::as_str) {
        Some("authorization_declined") => "Sign-in was declined in the browser.".into(),
        Some("expired_token") => "The sign-in code expired. Start again.".into(),
        Some("bad_verification_code") => "The sign-in code was not recognised. Start again.".into(),
        Some("invalid_grant") => SIGN_IN_AGAIN.into(),
        Some("invalid_client" | "unauthorized_client") => {
            "Microsoft refused this build's app registration (client ID).".into()
        }
        Some(other) => format!("Microsoft sign-in failed ({other})."),
        None => "Microsoft sign-in failed.".into(),
    }
}

/// Microsoft's tokens.
struct MsTokens {
    access: String,
    refresh: String,
}

fn ms_tokens(status: u16, v: &Value) -> Result<MsTokens, String> {
    match (
        status,
        str_field(v, "access_token"),
        str_field(v, "refresh_token"),
    ) {
        (200, Some(access), Some(refresh)) => Ok(MsTokens { access, refresh }),
        _ => Err(oauth_error(v)),
    }
}

/// Waits for the player to finish in the browser, polling as asked, until the code expires or
/// `cancelled` says stop; then signs in to Minecraft.
pub fn finish(
    client_id: &str,
    code: &DeviceCode,
    cancelled: &dyn Fn() -> bool,
) -> Result<Stored, String> {
    let mut interval = code.interval;
    let tokens = loop {
        let until = Instant::now() + Duration::from_secs(interval);
        while Instant::now() < until {
            if cancelled() {
                return Err("Sign-in cancelled.".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if now() > code.expires_at {
            return Err("The sign-in code expired. Start again.".into());
        }
        let (status, v) = send(
            request("POST", &format!("{MICROSOFT}/token")),
            Some(Body::Form(vec![
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", client_id),
                ("device_code", &code.device_code),
            ])),
            "Microsoft",
        )?;
        match v.get("error").and_then(Value::as_str) {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                interval += 5;
                continue;
            }
            _ => break ms_tokens(status, &v)?,
        }
    };
    if cancelled() {
        return Err("Sign-in cancelled.".into());
    }
    minecraft(tokens)
}

fn refresh_ms(client_id: &str, refresh: &str) -> Result<MsTokens, String> {
    let (status, v) = send(
        request("POST", &format!("{MICROSOFT}/token")),
        Some(Body::Form(vec![
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh),
            ("scope", SCOPE),
        ])),
        "Microsoft",
    )?;
    ms_tokens(status, &v)
}

/// The XSTS `XErr` codes, as the official launcher explains them.
fn xsts_error(code: u64) -> String {
    match code {
        2148916233 => "This Microsoft account has no Xbox profile yet. Sign in once at xbox.com \
                       to create one, then try again."
            .into(),
        2148916235 => "Xbox Live is not available in your country or region.".into(),
        2148916236 | 2148916237 => {
            "This account needs adult verification on xbox.com before it can play.".into()
        }
        2148916238 => {
            "This is a child account: an adult has to add it to a Microsoft family first.".into()
        }
        other => format!("Xbox Live refused the sign-in (error {other})."),
    }
}

/// The Minecraft token from the Microsoft one, through Xbox Live and XSTS.
fn minecraft_token(ms_access: &str) -> Result<(String, u64, Option<String>), String> {
    let (status, xbl) = send(
        request("POST", XBOX_USER),
        Some(Body::Json(json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={ms_access}"),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }))),
        "Xbox Live",
    )?;
    let xbl_token = str_field(&xbl, "Token")
        .filter(|_| status == 200)
        .ok_or("Xbox Live refused the sign-in.")?;
    let (status, xsts) = send(
        request("POST", XSTS),
        Some(Body::Json(json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }))),
        "Xbox Live",
    )?;
    if let Some(code) = xsts.get("XErr").and_then(Value::as_u64) {
        return Err(xsts_error(code));
    }
    let claims = xsts.pointer("/DisplayClaims/xui/0");
    let (Some(xsts_token), Some(uhs)) = (
        str_field(&xsts, "Token").filter(|_| status == 200),
        claims.and_then(|c| str_field(c, "uhs")),
    ) else {
        return Err("Xbox Live refused the sign-in.".into());
    };
    let xuid = claims.and_then(|c| str_field(c, "xid"));
    let (status, mc) = send(
        request(
            "POST",
            &format!("{MINECRAFT}/authentication/login_with_xbox"),
        ),
        Some(Body::Json(
            json!({ "identityToken": format!("XBL3.0 x={uhs};{xsts_token}") }),
        )),
        "the Minecraft services",
    )?;
    match (status, str_field(&mc, "access_token")) {
        (200, Some(token)) => {
            let expires = mc
                .get("expires_in")
                .and_then(Value::as_u64)
                .unwrap_or(86400);
            Ok((token, now() + expires, xuid))
        }
        // New Azure apps are refused until Mojang approves them (Minecraft API access).
        (403, _) => Err(
            "The Minecraft services refused this build's app registration: it is \
                         not approved by Mojang yet (ios/README.md, \"Microsoft sign-in\")."
                .into(),
        ),
        (429, _) => Err("Too many sign-ins in a short time. Wait a minute and try again.".into()),
        _ => Err("The Minecraft services refused the sign-in.".into()),
    }
}

fn bearer(request: ureq::Request, token: &str) -> ureq::Request {
    request.set("Authorization", &format!("Bearer {token}"))
}

/// Whether the account owns Minecraft: Java Edition (Game Pass included).
fn owns_game(token: &str) -> Result<bool, String> {
    let (status, v) = send(
        bearer(
            request("GET", &format!("{MINECRAFT}/entitlements/mcstore")),
            token,
        ),
        None,
        "the Minecraft services",
    )?;
    if status != 200 {
        return Err("Could not check which games this account owns.".into());
    }
    Ok(owns_in(&v))
}

/// Whether an `entitlements/mcstore` answer lists the game.
fn owns_in(v: &Value) -> bool {
    v.get("items")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|i| {
                matches!(
                    i.get("name").and_then(Value::as_str),
                    Some("game_minecraft" | "product_minecraft")
                )
            })
        })
}

/// The profile's UUID (no dashes) and name; none when the account has no profile.
fn profile(token: &str) -> Result<Option<(String, String)>, String> {
    let (status, v) = send(
        bearer(
            request("GET", &format!("{MINECRAFT}/minecraft/profile")),
            token,
        ),
        None,
        "the Minecraft services",
    )?;
    if status == 404 {
        return Ok(None);
    }
    match (str_field(&v, "id"), str_field(&v, "name")) {
        (Some(uuid), Some(name)) => Ok(Some((uuid.replace('-', "").to_lowercase(), name))),
        _ => Err("Could not read the Minecraft profile.".into()),
    }
}

/// The account as the keychain keeps it, between runs.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Stored {
    pub name: String,
    /// The profile UUID without dashes.
    pub uuid: String,
    #[serde(default)]
    pub xuid: Option<String>,
    refresh_token: String,
    minecraft_token: String,
    minecraft_expires_at: u64,
}
impl std::fmt::Debug for Stored {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stored")
            .field("name", &self.name)
            .field("uuid", &self.uuid)
            .finish_non_exhaustive()
    }
}

/// From Microsoft tokens to a ready account: Minecraft token, ownership, profile.
fn minecraft(ms: MsTokens) -> Result<Stored, String> {
    let (token, expires_at, xuid) = minecraft_token(&ms.access)?;
    if !owns_game(&token)? {
        return Err(DOES_NOT_OWN.into());
    }
    let (uuid, name) = profile(&token)?.ok_or(NO_PROFILE)?;
    Ok(Stored {
        name,
        uuid,
        xuid,
        refresh_token: ms.refresh,
        minecraft_token: token,
        minecraft_expires_at: expires_at,
    })
}

impl Stored {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
    pub fn from_json(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }
    /// Whether the Minecraft token still has an hour or more.
    pub fn fresh(&self) -> bool {
        self.minecraft_expires_at > now() + RENEW_BEFORE
    }
    /// A new Minecraft token through the refresh token (and the profile again: the name may
    /// have changed).
    pub fn renew(&self, client_id: &str) -> Result<Stored, String> {
        let ms = refresh_ms(client_id, &self.refresh_token)?;
        minecraft(ms)
    }
    /// The session the game joins servers with.
    pub fn session(&self) -> Option<crate::auth::AuthSession> {
        let text = json!({
            "name": self.name,
            "uuid": self.uuid,
            "access_token": self.minecraft_token,
            "user_type": "msa",
            "xuid": self.xuid,
        })
        .to_string();
        crate::auth::AuthSession::parse(&text).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownership_from_entitlements() {
        let owns =
            json!({ "items": [{ "name": "product_minecraft" }, { "name": "game_minecraft" }] });
        assert!(owns_in(&owns));
        assert!(!owns_in(&json!({ "items": [{ "name": "game_dungeons" }] })));
        assert!(!owns_in(&json!({})));
    }

    #[test]
    fn stored_round_trip_and_session() {
        let stored = Stored {
            name: "Steve".into(),
            uuid: "069a79f444e94726a5befca90e38aaf5".into(),
            xuid: Some("2535".into()),
            refresh_token: "secret-refresh".into(),
            minecraft_token: "secret-minecraft".into(),
            minecraft_expires_at: now() + 86400,
        };
        let back = Stored::from_json(&stored.to_json()).unwrap();
        assert_eq!(back.name, "Steve");
        assert!(back.fresh());
        let session = back.session().unwrap();
        assert_eq!(session.name, "Steve");
        assert_eq!(session.xuid.as_deref(), Some("2535"));
        // Tokens stay out of debug output.
        let debug = format!("{back:?}");
        assert!(debug.contains("Steve") && !debug.contains("secret"));
    }

    #[test]
    fn oauth_errors_read_as_messages() {
        assert_eq!(
            oauth_error(&json!({ "error": "expired_token" })),
            "The sign-in code expired. Start again."
        );
        assert!(oauth_error(&json!({ "error": "invalid_client" })).contains("client ID"));
    }
}
