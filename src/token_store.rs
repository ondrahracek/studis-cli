//! Persistent access tokens in platform-appropriate private storage.

pub(crate) trait TokenStore {
    fn load(&self) -> Result<Option<String>, &'static str>;
    fn save(&self, token: &str) -> Result<(), &'static str>;

    fn with_lock<T>(
        &self,
        operation: impl FnOnce() -> Result<T, &'static str>,
    ) -> Result<T, &'static str> {
        operation()
    }
}

#[cfg(windows)]
pub(crate) struct KeyringStore {
    entry: keyring::Entry,
}

#[cfg(windows)]
impl KeyringStore {
    pub(crate) fn new(client_uid: &str) -> Result<Self, &'static str> {
        let entry = keyring::Entry::new("studis-cli-vut-access-token", client_uid)
            .map_err(|_| "VUT token secure storage is unavailable")?;
        Ok(Self { entry })
    }
}

#[cfg(windows)]
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

#[cfg(unix)]
mod unix {
    use super::TokenStore;
    use crate::unix_acl::descriptor_has_no_extended_acl;
    use directories::ProjectDirs;
    use rustix::fs::{self as unix_fs, FileType, Mode, OFlags};
    use serde::{Deserialize, Serialize};
    use std::{
        collections::BTreeMap,
        fs::{self, File, TryLockError},
        io::{Read, Write},
        os::unix::fs::DirBuilderExt,
        path::Path,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    const CACHE_FILE: &str = "token-cache.json";
    const LOCK_FILE: &str = "token-cache.lock";
    const CACHE_VERSION: u8 = 1;
    const MAX_CACHE_BYTES: u64 = 1024 * 1024;
    const LOCK_TIMEOUT: Duration = Duration::from_secs(30);
    const LOCK_RETRY: Duration = Duration::from_millis(50);

    #[derive(Default, Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct TokenCache {
        version: u8,
        tokens: BTreeMap<String, String>,
    }

    pub(crate) struct FileTokenStore {
        directory: File,
        client_uid: String,
    }

    impl FileTokenStore {
        pub(crate) fn new(client_uid: &str) -> Result<Self, &'static str> {
            let project = ProjectDirs::from("", "", "studis-cli")
                .ok_or("VUT token file storage is unavailable")?;
            #[cfg(target_os = "macos")]
            let directory = project.data_local_dir();
            #[cfg(not(target_os = "macos"))]
            let directory = project.state_dir().unwrap_or(project.data_local_dir());
            Self::open_in(directory, client_uid)
        }

        #[cfg(test)]
        pub(crate) fn new_in(directory: &Path, client_uid: &str) -> Result<Self, &'static str> {
            Self::open_in(directory, client_uid)
        }

        fn open_in(directory: &Path, client_uid: &str) -> Result<Self, &'static str> {
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true).mode(0o700);
            builder
                .create(directory)
                .map_err(|_| "unable to create VUT token cache directory")?;

            let directory = unix_fs::open(
                directory,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map(File::from)
            .map_err(|_| "VUT token cache directory is unsafe")?;
            validate_descriptor(&directory, FileType::Directory, Mode::RWXU)
                .map_err(|_| "VUT token cache directory is unsafe")?;

            Ok(Self {
                directory,
                client_uid: client_uid.to_owned(),
            })
        }

        fn open_cache(&self) -> Result<Option<File>, &'static str> {
            match unix_fs::openat(
                &self.directory,
                CACHE_FILE,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            ) {
                Ok(file) => {
                    let file = File::from(file);
                    validate_descriptor(&file, FileType::RegularFile, Mode::RUSR | Mode::WUSR)
                        .map_err(|_| "VUT token cache file is unsafe")?;
                    Ok(Some(file))
                }
                Err(rustix::io::Errno::NOENT) => Ok(None),
                Err(_) => Err("VUT token cache file is unsafe"),
            }
        }

        fn read_cache(&self) -> Result<Option<TokenCache>, &'static str> {
            let Some(file) = self.open_cache()? else {
                return Ok(None);
            };
            let mut bytes = Vec::new();
            file.take(MAX_CACHE_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "unable to read VUT token cache")?;
            if bytes.len() as u64 > MAX_CACHE_BYTES {
                return Err("VUT token cache is invalid");
            }
            let cache: TokenCache =
                serde_json::from_slice(&bytes).map_err(|_| "VUT token cache is invalid")?;
            if cache.version != CACHE_VERSION || cache.tokens.values().any(|token| token.is_empty())
            {
                return Err("VUT token cache is invalid");
            }
            Ok(Some(cache))
        }

        fn write_cache(&self, cache: &TokenCache) -> Result<(), &'static str> {
            self.write_cache_before_rename(cache, || Ok(()))
        }

        fn write_cache_before_rename(
            &self,
            cache: &TokenCache,
            before_rename: impl FnOnce() -> Result<(), &'static str>,
        ) -> Result<(), &'static str> {
            let bytes = serde_json::to_vec(cache).map_err(|_| "unable to save VUT token cache")?;
            if bytes.len() as u64 > MAX_CACHE_BYTES {
                return Err("VUT token cache is too large");
            }
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "unable to save VUT token cache")?
                .as_nanos();
            let temporary = format!(".token-cache-{}-{nonce}.tmp", std::process::id());
            let result = (|| {
                let temporary_file = unix_fs::openat(
                    &self.directory,
                    temporary.as_str(),
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::RUSR | Mode::WUSR,
                )
                .map(File::from)
                .map_err(|_| "unable to save VUT token cache")?;
                validate_descriptor(
                    &temporary_file,
                    FileType::RegularFile,
                    Mode::RUSR | Mode::WUSR,
                )
                .map_err(|_| "VUT token cache file is unsafe")?;
                (&temporary_file)
                    .write_all(&bytes)
                    .map_err(|_| "unable to save VUT token cache")?;
                temporary_file
                    .sync_all()
                    .map_err(|_| "unable to save VUT token cache")?;
                before_rename()?;
                unix_fs::renameat(
                    &self.directory,
                    temporary.as_str(),
                    &self.directory,
                    CACHE_FILE,
                )
                .map_err(|_| "unable to save VUT token cache")?;
                self.directory
                    .sync_all()
                    .map_err(|_| "unable to save VUT token cache")
            })();
            if result.is_err() {
                let _ = unix_fs::unlinkat(
                    &self.directory,
                    temporary.as_str(),
                    unix_fs::AtFlags::empty(),
                );
            }
            result
        }

        #[cfg(test)]
        fn write_cache_interrupted(&self, cache: &TokenCache) -> Result<(), &'static str> {
            self.write_cache_before_rename(cache, || Err("injected token cache write failure"))
        }

        fn open_lock(&self) -> Result<File, &'static str> {
            let flags = OFlags::RDWR | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
            let mut opened = None;
            // A concurrent creator can make the first lookup race on macOS.
            for _ in 0..3 {
                match unix_fs::openat(&self.directory, LOCK_FILE, flags, Mode::empty()) {
                    Ok(file) => {
                        opened = Some(File::from(file));
                        break;
                    }
                    Err(rustix::io::Errno::NOENT) => {
                        match unix_fs::openat(
                            &self.directory,
                            LOCK_FILE,
                            flags | OFlags::CREATE | OFlags::EXCL,
                            Mode::RUSR | Mode::WUSR,
                        ) {
                            Ok(file) => {
                                opened = Some(File::from(file));
                                break;
                            }
                            Err(rustix::io::Errno::EXIST | rustix::io::Errno::NOENT) => continue,
                            Err(_) => return Err("VUT token cache lock is unsafe"),
                        }
                    }
                    Err(_) => return Err("VUT token cache lock is unsafe"),
                }
            }
            let file = opened.ok_or("VUT token cache lock is unsafe")?;
            validate_descriptor(&file, FileType::RegularFile, Mode::RUSR | Mode::WUSR)
                .map_err(|_| "VUT token cache lock is unsafe")?;
            Ok(file)
        }

        fn acquire_lock(&self) -> Result<File, &'static str> {
            let lock = self.open_lock()?;
            let deadline = Instant::now() + LOCK_TIMEOUT;
            loop {
                match lock.try_lock() {
                    Ok(()) => return Ok(lock),
                    Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                        std::thread::sleep(LOCK_RETRY);
                    }
                    Err(TryLockError::WouldBlock) => {
                        return Err("timed out waiting for VUT token cache lock");
                    }
                    Err(TryLockError::Error(_)) => {
                        return Err("unable to lock VUT token cache");
                    }
                }
            }
        }
    }

    impl TokenStore for FileTokenStore {
        fn load(&self) -> Result<Option<String>, &'static str> {
            Ok(self
                .read_cache()?
                .and_then(|cache| cache.tokens.get(&self.client_uid).cloned()))
        }

        fn save(&self, token: &str) -> Result<(), &'static str> {
            if token.is_empty() {
                return Err("VUT cached access token is invalid");
            }
            let mut cache = self.read_cache()?.unwrap_or(TokenCache {
                version: CACHE_VERSION,
                tokens: BTreeMap::new(),
            });
            cache
                .tokens
                .insert(self.client_uid.clone(), token.to_owned());
            self.write_cache(&cache)
        }

        fn with_lock<T>(
            &self,
            operation: impl FnOnce() -> Result<T, &'static str>,
        ) -> Result<T, &'static str> {
            let _lock = self.acquire_lock()?;
            operation()
        }
    }

    fn validate_descriptor(
        file: &File,
        expected_type: FileType,
        expected_mode: Mode,
    ) -> Result<(), ()> {
        let metadata = unix_fs::fstat(file).map_err(|_| ())?;
        if !metadata_is_safe(
            &metadata,
            expected_type,
            expected_mode,
            rustix::process::geteuid().as_raw(),
        ) {
            return Err(());
        }
        if !descriptor_has_no_extended_acl(file)? {
            return Err(());
        }
        Ok(())
    }

    fn metadata_is_safe(
        metadata: &unix_fs::Stat,
        expected_type: FileType,
        expected_mode: Mode,
        expected_uid: u32,
    ) -> bool {
        FileType::from_raw_mode(metadata.st_mode) == expected_type
            && Mode::from_raw_mode(metadata.st_mode) == expected_mode
            && metadata.st_uid == expected_uid
            && (expected_type != FileType::RegularFile || metadata.st_nlink == 1)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{
            os::unix::fs::{MetadataExt, PermissionsExt, symlink},
            path::PathBuf,
            process::Command,
            time::{SystemTime, UNIX_EPOCH},
        };

        struct TestDirectory(PathBuf);

        impl TestDirectory {
            fn new(label: &str) -> Self {
                let nonce = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock after epoch")
                    .as_nanos();
                let path = std::env::temp_dir().join(format!(
                    "studis-token-store-{label}-{}-{nonce}",
                    std::process::id()
                ));
                Self(path)
            }

            fn create_private(&self) {
                fs::create_dir(&self.0).expect("create test directory");
                fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700))
                    .expect("set private test directory mode");
            }
        }

        impl Drop for TestDirectory {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        #[cfg(target_os = "macos")]
        fn add_acl(path: &Path, ace: &str) {
            let output = Command::new("/bin/chmod")
                .args(["+a", ace])
                .arg(path)
                .output()
                .expect("run chmod for synthetic ACL");
            assert!(
                output.status.success(),
                "chmod failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        #[test]
        fn private_store_round_trips_tokens_with_private_modes() {
            let directory = TestDirectory::new("round-trip");
            let store =
                FileTokenStore::new_in(&directory.0, "synthetic-uid").expect("create store");

            assert_eq!(store.load().expect("empty cache"), None);
            store.save("synthetic-token").expect("save token");
            assert_eq!(
                store.load().expect("load token").as_deref(),
                Some("synthetic-token")
            );

            let directory_mode = fs::metadata(&directory.0)
                .expect("cache directory metadata")
                .permissions()
                .mode()
                & 0o777;
            let file_mode = fs::metadata(directory.0.join(CACHE_FILE))
                .expect("cache file metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(directory_mode, 0o700);
            assert_eq!(file_mode, 0o600);
        }

        #[test]
        fn descriptor_owner_must_match_effective_user() {
            let directory = TestDirectory::new("owner");
            let store = FileTokenStore::new_in(&directory.0, "uid").expect("create store");
            store.save("synthetic-token").expect("save token");
            let cache = store.open_cache().expect("open cache").expect("cache file");
            let metadata = unix_fs::fstat(cache).expect("cache metadata");

            assert!(metadata_is_safe(
                &metadata,
                FileType::RegularFile,
                Mode::RUSR | Mode::WUSR,
                metadata.st_uid
            ));
            assert!(!metadata_is_safe(
                &metadata,
                FileType::RegularFile,
                Mode::RUSR | Mode::WUSR,
                metadata.st_uid.wrapping_add(1)
            ));
        }

        #[cfg(target_os = "macos")]
        #[test]
        fn extended_acl_on_cache_directory_is_rejected() {
            let directory = TestDirectory::new("directory-acl");
            directory.create_private();
            add_acl(&directory.0, "everyone allow read");

            assert!(FileTokenStore::new_in(&directory.0, "uid").is_err());
        }

        #[cfg(target_os = "macos")]
        #[test]
        fn extended_acl_on_cache_file_is_rejected() {
            let directory = TestDirectory::new("cache-acl");
            let store = FileTokenStore::new_in(&directory.0, "uid").expect("create store");
            store.save("synthetic-token").expect("save token");
            add_acl(&directory.0.join(CACHE_FILE), "everyone allow read");

            assert!(store.load().is_err());
        }

        #[cfg(target_os = "macos")]
        #[test]
        fn extended_acl_on_lock_file_is_rejected() {
            let directory = TestDirectory::new("lock-acl");
            let store = FileTokenStore::new_in(&directory.0, "uid").expect("create store");
            store.with_lock(|| Ok(())).expect("create lock");
            add_acl(&directory.0.join(LOCK_FILE), "everyone allow read");

            assert!(store.with_lock(|| Ok(())).is_err());
        }

        #[cfg(target_os = "macos")]
        #[test]
        fn inherited_acl_on_temporary_file_prevents_replacement() {
            let directory = TestDirectory::new("temporary-acl");
            let store = FileTokenStore::new_in(&directory.0, "uid").expect("create store");
            store.save("old-token").expect("save old token");
            add_acl(
                &directory.0,
                "everyone allow read,file_inherit,only_inherit",
            );

            assert!(store.save("new-token").is_err());
            assert_eq!(
                store.load().expect("old cache").as_deref(),
                Some("old-token")
            );
        }

        #[test]
        fn one_cache_preserves_tokens_for_multiple_client_uids() {
            let directory = TestDirectory::new("multiple-uids");
            let first = FileTokenStore::new_in(&directory.0, "first-uid").expect("first store");
            first.save("first-token").expect("save first token");
            let second = FileTokenStore::new_in(&directory.0, "second-uid").expect("second store");
            second.save("second-token").expect("save second token");

            assert_eq!(
                first.load().expect("first token").as_deref(),
                Some("first-token")
            );
            assert_eq!(
                second.load().expect("second token").as_deref(),
                Some("second-token")
            );
        }

        #[test]
        fn unsafe_or_symlinked_cache_directory_is_rejected() {
            let target = TestDirectory::new("directory-target");
            target.create_private();
            let link = TestDirectory::new("directory-link");
            symlink(&target.0, &link.0).expect("create directory symlink");
            assert!(FileTokenStore::new_in(&link.0, "uid").is_err());

            let unsafe_directory = TestDirectory::new("unsafe-directory");
            fs::create_dir(&unsafe_directory.0).expect("create unsafe directory");
            fs::set_permissions(&unsafe_directory.0, fs::Permissions::from_mode(0o755))
                .expect("set unsafe mode");
            assert!(FileTokenStore::new_in(&unsafe_directory.0, "uid").is_err());
        }

        #[test]
        fn unsafe_nonregular_and_symlinked_cache_files_are_rejected() {
            for case in ["directory", "symlink", "hard-link", "unsafe-mode"] {
                let directory = TestDirectory::new(case);
                let store = FileTokenStore::new_in(&directory.0, "uid").expect("create store");
                let cache_path = directory.0.join(CACHE_FILE);
                match case {
                    "unsafe-mode" => {
                        fs::write(&cache_path, br#"{"version":1,"tokens":{}}"#)
                            .expect("write cache");
                        fs::set_permissions(&cache_path, fs::Permissions::from_mode(0o644))
                            .expect("set unsafe cache mode");
                    }
                    "directory" => fs::create_dir(&cache_path).expect("create cache directory"),
                    "symlink" => {
                        let target = directory.0.join("symlink-target");
                        fs::write(&target, br#"{"version":1,"tokens":{}}"#)
                            .expect("write symlink target");
                        fs::set_permissions(&target, fs::Permissions::from_mode(0o600))
                            .expect("set symlink target mode");
                        symlink("symlink-target", &cache_path).expect("create symlink");
                    }
                    "hard-link" => {
                        let target = directory.0.join("target");
                        fs::write(&target, br#"{"version":1,"tokens":{}}"#)
                            .expect("write hard-link target");
                        fs::set_permissions(&target, fs::Permissions::from_mode(0o600))
                            .expect("set target mode");
                        fs::hard_link(&target, &cache_path).expect("create hard link");
                    }
                    _ => unreachable!(),
                }
                assert!(store.load().is_err(), "case {case} must fail");
            }
        }

        #[test]
        fn unsafe_or_symlinked_lock_file_is_rejected() {
            for case in ["symlink", "unsafe-mode"] {
                let directory = TestDirectory::new(case);
                let store = FileTokenStore::new_in(&directory.0, "uid").expect("create store");
                let lock_path = directory.0.join(LOCK_FILE);
                match case {
                    "unsafe-mode" => {
                        fs::write(&lock_path, b"").expect("write lock");
                        fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o644))
                            .expect("set unsafe lock mode");
                    }
                    "symlink" => {
                        let target = directory.0.join("lock-target");
                        fs::write(&target, b"").expect("write lock target");
                        fs::set_permissions(&target, fs::Permissions::from_mode(0o600))
                            .expect("set lock target mode");
                        symlink("lock-target", &lock_path).expect("create symlink");
                    }
                    _ => unreachable!(),
                }
                assert!(store.with_lock(|| Ok(())).is_err(), "case {case} must fail");
            }
        }

        #[test]
        fn failure_before_atomic_replacement_keeps_old_cache_and_stable_lock() {
            let directory = TestDirectory::new("atomic");
            let store = FileTokenStore::new_in(&directory.0, "uid").expect("create store");
            store
                .with_lock(|| store.save("old-token"))
                .expect("initial save");
            let lock_inode = fs::metadata(directory.0.join(LOCK_FILE))
                .expect("lock metadata")
                .ino();
            let lock_mode = fs::metadata(directory.0.join(LOCK_FILE))
                .expect("lock metadata")
                .permissions()
                .mode()
                & 0o777;
            let old_cache_inode = fs::metadata(directory.0.join(CACHE_FILE))
                .expect("old cache metadata")
                .ino();
            assert_eq!(lock_mode, 0o600);

            let replacement = TokenCache {
                version: CACHE_VERSION,
                tokens: BTreeMap::from([("uid".to_owned(), "new-token".to_owned())]),
            };
            assert_eq!(
                store.write_cache_interrupted(&replacement),
                Err("injected token cache write failure")
            );
            assert!(
                fs::read_dir(&directory.0)
                    .expect("list cache directory")
                    .all(|entry| !entry
                        .expect("cache directory entry")
                        .file_name()
                        .to_string_lossy()
                        .ends_with(".tmp"))
            );
            assert_eq!(
                store.load().expect("old cache").as_deref(),
                Some("old-token")
            );

            store
                .with_lock(|| store.save("new-token"))
                .expect("replace cache");
            assert_eq!(
                store.load().expect("new cache").as_deref(),
                Some("new-token")
            );
            assert_eq!(
                fs::metadata(directory.0.join(LOCK_FILE))
                    .expect("lock metadata after replacement")
                    .ino(),
                lock_inode
            );
            assert_ne!(
                fs::metadata(directory.0.join(CACHE_FILE))
                    .expect("new cache metadata")
                    .ino(),
                old_cache_inode
            );
        }

        fn run_process_helper(directory: &Path, action: &str) {
            let output = Command::new(std::env::current_exe().expect("current test executable"))
                .args([
                    "--ignored",
                    "--exact",
                    "token_store::unix::tests::process_helper",
                ])
                .env("STUDIS_TOKEN_TEST_DIRECTORY", directory)
                .env("STUDIS_TOKEN_TEST_ACTION", action)
                .output()
                .expect("run token-store helper");
            assert!(
                output.status.success(),
                "helper failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        #[test]
        fn token_cache_is_reused_across_processes() {
            let directory = TestDirectory::new("cross-process");
            run_process_helper(&directory.0, "write");
            run_process_helper(&directory.0, "read");
        }

        #[test]
        #[ignore = "child-process helper"]
        fn process_helper() {
            let directory = PathBuf::from(
                std::env::var_os("STUDIS_TOKEN_TEST_DIRECTORY").expect("helper cache directory"),
            );
            let action = std::env::var("STUDIS_TOKEN_TEST_ACTION").expect("helper action");
            let store = FileTokenStore::new_in(&directory, "synthetic-uid").expect("helper store");
            match action.as_str() {
                "write" => store.save("synthetic-token").expect("helper save"),
                "read" => assert_eq!(
                    store.load().expect("helper load").as_deref(),
                    Some("synthetic-token")
                ),
                _ => panic!("unknown helper action"),
            }
        }
    }
}

#[cfg(windows)]
pub(crate) use KeyringStore as PlatformTokenStore;
#[cfg(unix)]
pub(crate) use unix::FileTokenStore as PlatformTokenStore;

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    const SERVICE: &str = "studis-cli-vut-access-token";
    const TOKEN: &str = "synthetic-windows-ci-token";

    struct CredentialCleanup(String);

    impl Drop for CredentialCleanup {
        fn drop(&mut self) {
            if let Ok(entry) = keyring::Entry::new(SERVICE, &self.0) {
                let _ = entry.delete_credential();
            }
        }
    }

    fn run_helper(uid: &str, action: &str) {
        let mut child = Command::new(std::env::current_exe().expect("current test executable"))
            .args([
                "--ignored",
                "--exact",
                "token_store::windows_tests::credential_manager_process_helper",
            ])
            .env("STUDIS_WINDOWS_CREDENTIAL_TEST_UID", uid)
            .env("STUDIS_WINDOWS_CREDENTIAL_TEST_ACTION", action)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run Credential Manager helper");
        let deadline = Instant::now() + Duration::from_secs(15);
        while child
            .try_wait()
            .expect("poll Credential Manager helper")
            .is_none()
        {
            if Instant::now() >= deadline {
                child
                    .kill()
                    .expect("stop timed-out Credential Manager helper");
                let _ = child.wait();
                panic!("Credential Manager helper timed out");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let output = child
            .wait_with_output()
            .expect("collect Credential Manager helper output");
        assert!(
            output.status.success(),
            "Credential Manager helper failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn credential_manager_round_trips_across_processes() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let uid = format!("studis-ci-{}-{nonce}", std::process::id());
        let _cleanup = CredentialCleanup(uid.clone());

        run_helper(&uid, "write");
        run_helper(&uid, "read");

        let entry = keyring::Entry::new(SERVICE, &uid).expect("test Credential Manager entry");
        entry
            .delete_credential()
            .expect("delete synthetic Credential Manager entry");
        assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
    }

    #[test]
    #[ignore = "child-process helper"]
    fn credential_manager_process_helper() {
        let uid = std::env::var("STUDIS_WINDOWS_CREDENTIAL_TEST_UID").expect("test uid");
        let action = std::env::var("STUDIS_WINDOWS_CREDENTIAL_TEST_ACTION").expect("test action");
        let store = KeyringStore::new(&uid).expect("Credential Manager store");
        match action.as_str() {
            "write" => store.save(TOKEN).expect("write synthetic credential"),
            "read" => assert_eq!(
                store.load().expect("read credential").as_deref(),
                Some(TOKEN)
            ),
            _ => panic!("unknown helper action"),
        }
    }
}
