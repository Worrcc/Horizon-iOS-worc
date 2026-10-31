//! Secrets in the system's keychain (the Microsoft sign-in's refresh token): never in the game
//! folder, which the Files app shows. Everything here does nothing on other systems.

const SERVICE: &str = "dev.horizon.ios";

/// The secret kept under `key`, if any.
pub fn get(key: &str) -> Option<String> {
    #[cfg(target_os = "ios")]
    return security_framework::passwords::get_generic_password(SERVICE, key)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok());
    #[cfg(not(target_os = "ios"))]
    {
        let _ = (SERVICE, key);
        None
    }
}

/// Keeps `secret` under `key`, replacing what was there.
pub fn set(key: &str, secret: &str) -> Result<(), String> {
    #[cfg(target_os = "ios")]
    return security_framework::passwords::set_generic_password(SERVICE, key, secret.as_bytes())
        .map_err(|e| format!("the keychain refused it ({e})"));
    #[cfg(not(target_os = "ios"))]
    {
        let _ = (key, secret);
        Err("no keychain on this system".into())
    }
}

/// Forgets the secret under `key`.
pub fn remove(key: &str) {
    #[cfg(target_os = "ios")]
    let _ = security_framework::passwords::delete_generic_password(SERVICE, key);
    #[cfg(not(target_os = "ios"))]
    let _ = key;
}
