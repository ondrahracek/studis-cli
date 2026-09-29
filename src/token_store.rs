//! Persistent access tokens in the operating system credential store.

pub(crate) trait TokenStore {
    fn load(&self) -> Result<Option<String>, &'static str>;
    fn save(&self, token: &str) -> Result<(), &'static str>;
}

pub(crate) struct KeyringStore {
    entry: keyring::Entry,
}

impl KeyringStore {
    pub(crate) fn new(client_uid: &str) -> Result<Self, &'static str> {
        let entry = keyring::Entry::new("studis-cli-vut-access-token", client_uid)
            .map_err(|_| "VUT token secure storage is unavailable")?;
        Ok(Self { entry })
    }
}

impl TokenStore for KeyringStore {
    fn load(&self) -> Result<Option<String>, &'static str> {
        match self.entry.get_password() {
            Ok(token) if token.is_empty() => Err("VUT cached access token is invalid"),
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("unable to read VUT token from secure storage"),
        }
    }

    fn save(&self, token: &str) -> Result<(), &'static str> {
        self.entry
            .set_password(token)
            .map_err(|_| "unable to save VUT token to secure storage")
    }
}
