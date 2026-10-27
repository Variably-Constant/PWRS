//! `cargo pwrs sign`: Authenticode signatures on a module folder's
//! scripts and Windows executables, and a signed catalog over all its
//! files when the settings set `catalog`, made in pwsh on any platform by
//! Pwrs.Signing or by OpenAuthenticode. The settings are
//! `[package.metadata.pwrs.signing]`, with a flag of the same name over
//! each one.

use std::path::{Path, PathBuf};

use crate::Error;
use pwrs_build::{pwsh, toolchain, DebugSymbols, Toolchain};

const SIGNING_SOURCES: &[(&str, &str)] = &[
    ("Cms.cs", include_str!("../dotnet/Pwrs.Signing/Cms.cs")),
    ("Keys.cs", include_str!("../dotnet/Pwrs.Signing/Keys.cs")),
    ("Pkcs11.cs", include_str!("../dotnet/Pwrs.Signing/Pkcs11.cs")),
    ("Scripts.cs", include_str!("../dotnet/Pwrs.Signing/Scripts.cs")),
    ("Pe.cs", include_str!("../dotnet/Pwrs.Signing/Pe.cs")),
    ("Catalog.cs", include_str!("../dotnet/Pwrs.Signing/Catalog.cs")),
    ("Signer.cs", include_str!("../dotnet/Pwrs.Signing/Signer.cs")),
];

const SIGN_PS1: &str = include_str!("../scripts/sign.ps1");

/// The settings, in the order messages list them.
pub const SETTINGS: [&str; 11] = ["pfx", "pem", "store", "command", "pkcs11", "password-env", "certificate", "timestamp-url", "hash", "catalog", "signer"];

/// The settings that name where the key is, of which a signing names one.
const SOURCES: [&str; 5] = ["pfx", "pem", "store", "command", "pkcs11"];

/// The placeholders that give a signing command the digest to sign.
const DIGEST_PLACEHOLDERS: [&str; 3] = ["{digest}", "{digest-base64}", "{digest-hex}"];

/// Where the signing key is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    /// A PKCS#12 file with the certificate and its key.
    Pfx(PathBuf),
    /// A PEM file of certificates, the signer's among them, and a PEM file
    /// with its private key.
    Pem { certificate: PathBuf, key: PathBuf },
    /// A certificate with its key in the Windows certificate store or the
    /// macOS keychain, by its thumbprint.
    Store(String),
    /// A program that signs a digest, with its arguments, run in `dir`:
    /// the folder its setting came from, which a relative program path is
    /// also taken from.
    Command { args: Vec<String>, dir: PathBuf },
    /// A private key on a PKCS#11 token: the token's library, the token's
    /// label and the key's label.
    Pkcs11 { library: PathBuf, token: String, key: String },
}

/// What a signing uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub source: KeySource,
    /// The name of the variable set to the PFX password, the PEM key's
    /// password or the token's PIN.
    pub password_env: Option<String>,
    /// The certificates of a command or PKCS#11 key: the signer's and the
    /// chain to include with it.
    pub certificate: Option<PathBuf>,
    pub timestamp_url: Option<String>,
    /// `sha256`, `sha384` or `sha512`.
    pub hash: String,
    pub catalog: bool,
    /// Whether OpenAuthenticode signs the scripts and executables.
    pub openauthenticode: bool,
}

/// One setting's value and where it came from: the folder its paths are
/// joined to, and how a message names it.
struct Given {
    value: serde_json::Value,
    base: PathBuf,
    name: String,
}

/// A flag's value as JSON: `catalog` with no value is true, `pem`,
/// `command` and `pkcs11` take their value as Cargo.toml writes it, and
/// every other setting takes its text.
pub fn flag_value(setting: &str, text: Option<&str>) -> Result<serde_json::Value, Error> {
    match (setting, text) {
        ("catalog", None) => Ok(serde_json::Value::Bool(true)),
        ("catalog", Some(t)) => Err(Error::msg(format!("--catalog takes no value, and {t:?} follows it; leave catalog out to make no catalog"))),
        ("pem" | "command" | "pkcs11", Some(t)) => {
            let table: toml::Table = toml::from_str(&format!("value = {t}"))
                .map_err(|e| Error::msg(format!("--{setting} takes its value as Cargo.toml writes it, such as {}, and {t:?} is not one: {e}", example(setting))))?;
            let value = table.get("value").cloned().ok_or_else(|| Error::msg(format!("--{setting} has no value")))?;
            serde_json::to_value(value).map_err(|e| Error::msg(format!("--{setting}: {e}")))
        }
        (_, Some(t)) => Ok(serde_json::Value::String(t.to_string())),
        (_, None) => Err(Error::msg(format!("--{setting} takes a value"))),
    }
}

fn example(setting: &str) -> &'static str {
    match setting {
        "pem" => r#"'["signer.pem", "signer.key"]'"#,
        "command" => r#"'["aws", "kms", "sign", "--message", "fileb://{digest}"]'"#,
        _ => r#"'{ library = "/usr/lib/softhsm/libsofthsm2.so", token = "signing", key = "code" }'"#,
    }
}

/// The settings `table` gives, `[package.metadata.pwrs.signing]` as
/// cargo metadata reports it (null when the crate has none), with
/// `flags` over them. A flag that names a key source replaces the
/// table's. Paths in the table are joined to `crate_dir`, and paths in
/// flags to `flag_dir`. None when neither gives any setting.
pub fn settings(table: &serde_json::Value, crate_dir: &Path, flags: &[(String, serde_json::Value)], flag_dir: &Path) -> Result<Option<Settings>, Error> {
    let at = "[package.metadata.pwrs.signing]";
    let mut given: Vec<(String, Given)> = Vec::new();
    match table {
        serde_json::Value::Null => {}
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if !SETTINGS.contains(&key.as_str()) {
                    return Err(Error::msg(format!("{at} sets `{key}`, which is not one of {}", SETTINGS.join(", "))));
                }
                given.push((key.clone(), Given { value: value.clone(), base: crate_dir.to_path_buf(), name: format!("{at} {key}") }));
            }
        }
        other => return Err(Error::msg(format!("{at} is {other}; it is a table of the signing settings"))),
    }
    if flags.iter().any(|(k, _)| SOURCES.contains(&k.as_str())) {
        given.retain(|(k, _)| !SOURCES.contains(&k.as_str()));
    }
    for (key, value) in flags {
        given.retain(|(k, _)| k != key);
        given.push((key.clone(), Given { value: value.clone(), base: flag_dir.to_path_buf(), name: format!("--{key}") }));
    }
    if given.is_empty() {
        return Ok(None);
    }
    let get = |key: &str| given.iter().find(|(k, _)| k == key).map(|(_, g)| g);
    let sources: Vec<&str> = SOURCES.iter().copied().filter(|s| get(s).is_some()).collect();
    let source = match sources.as_slice() {
        [one] => key_source(one, get(one).ok_or_else(|| Error::msg("a key source vanished"))?)?,
        [] => return Err(Error::msg(format!("the signing settings name no key: give one of {}", SOURCES.join(", ")))),
        many => return Err(Error::msg(format!("the signing settings name {} keys, {}; give one", many.len(), many.join(" and ")))),
    };
    let password_env = match get("password-env") {
        None => None,
        Some(g) => {
            let name = text(g)?;
            if name.contains('=') || name.chars().any(char::is_whitespace) {
                return Err(Error::msg(format!("{} is {name:?}; give the name of the environment variable set to the secret, not the secret itself", g.name)));
            }
            Some(name)
        }
    };
    let certificate = match get("certificate") {
        None => None,
        Some(g) => Some(g.base.join(text(g)?)),
    };
    let takes_certificate = matches!(source, KeySource::Command { .. } | KeySource::Pkcs11 { .. });
    match (&certificate, takes_certificate) {
        (None, true) => return Err(Error::msg(format!("a {} key needs certificate: a PEM, DER or PKCS#7 file with the signer's certificate and the chain to include", sources[0]))),
        (Some(_), false) => return Err(Error::msg(format!("certificate gives the certificates of a command or pkcs11 key; a {} key comes with its own", sources[0]))),
        _ => {}
    }
    let timestamp_url = match get("timestamp-url") {
        None => None,
        Some(g) => {
            let url = text(g)?;
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(Error::msg(format!("{} is {url:?}; it is the http or https address of an RFC 3161 timestamp server", g.name)));
            }
            Some(url)
        }
    };
    let hash = match get("hash") {
        None => String::from("sha256"),
        Some(g) => {
            let h = text(g)?;
            if !["sha256", "sha384", "sha512"].contains(&h.as_str()) {
                return Err(Error::msg(format!("{} is {h:?}; it takes sha256, sha384 or sha512", g.name)));
            }
            h
        }
    };
    let catalog = match get("catalog") {
        None => false,
        Some(g) => g.value.as_bool().ok_or_else(|| Error::msg(format!("{} is {}; it takes true or false", g.name, g.value)))?,
    };
    let openauthenticode = match get("signer") {
        None => false,
        Some(g) => match text(g)?.as_str() {
            "pwrs" => false,
            "openauthenticode" => true,
            other => return Err(Error::msg(format!("{} is {other:?}; it takes \"pwrs\" or \"openauthenticode\"", g.name))),
        },
    };
    if openauthenticode && takes_certificate {
        return Err(Error::msg(format!(
            "signer \"openauthenticode\" cannot sign with a {} key: OpenAuthenticode takes only a certificate that has its key. Give pfx, pem or store, or leave signer at \"pwrs\"",
            sources[0]
        )));
    }
    Ok(Some(Settings { source, password_env, certificate, timestamp_url, hash, catalog, openauthenticode }))
}

fn text(g: &Given) -> Result<String, Error> {
    match &g.value {
        serde_json::Value::String(s) if !s.trim().is_empty() => Ok(s.clone()),
        other => Err(Error::msg(format!("{} is {other}; it takes text", g.name))),
    }
}

fn texts(g: &Given) -> Result<Vec<String>, Error> {
    match &g.value {
        serde_json::Value::Array(items) => items
            .iter()
            .map(|v| match v {
                serde_json::Value::String(s) => Ok(s.clone()),
                other => Err(Error::msg(format!("{} lists {other}, which is not text", g.name))),
            })
            .collect(),
        other => Err(Error::msg(format!("{} is {other}; it takes a list of text", g.name))),
    }
}

fn key_source(source: &str, g: &Given) -> Result<KeySource, Error> {
    match source {
        "pfx" => Ok(KeySource::Pfx(g.base.join(text(g)?))),
        "pem" => match texts(g)?.as_slice() {
            [certificate, key] if !certificate.is_empty() && !key.is_empty() => Ok(KeySource::Pem { certificate: g.base.join(certificate), key: g.base.join(key) }),
            _other => Err(Error::msg(format!("{} takes two files, the certificates and the key, such as [\"signer.pem\", \"signer.key\"]", g.name))),
        },
        "store" => {
            let thumbprint: String = text(g)?.chars().filter(|c| !c.is_whitespace()).collect();
            if thumbprint.len() != 40 || !thumbprint.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(Error::msg(format!("{} is {thumbprint:?}; it takes a certificate's thumbprint, 40 hexadecimal digits", g.name)));
            }
            Ok(KeySource::Store(thumbprint.to_ascii_uppercase()))
        }
        "command" => {
            let command = texts(g)?;
            if command.first().is_none_or(|p| p.trim().is_empty()) {
                return Err(Error::msg(format!("{} names no program; it takes the program and its arguments, such as {}", g.name, example("command"))));
            }
            if !command.iter().any(|a| DIGEST_PLACEHOLDERS.iter().any(|p| a.contains(p))) {
                return Err(Error::msg(format!(
                    "{} names none of {}, so the command is never given the digest to sign",
                    g.name,
                    DIGEST_PLACEHOLDERS.join(", ")
                )));
            }
            Ok(KeySource::Command { args: command, dir: g.base.clone() })
        }
        _pkcs11 => {
            let map = g.value.as_object().ok_or_else(|| Error::msg(format!("{} is {}; it takes a table with library, token and key, such as {}", g.name, g.value, example("pkcs11"))))?;
            if let Some(stray) = map.keys().find(|k| !["library", "token", "key"].contains(&k.as_str())) {
                return Err(Error::msg(format!("{} sets `{stray}`, which is not one of library, token, key", g.name)));
            }
            let field = |name: &str| -> Result<String, Error> {
                match map.get(name) {
                    Some(serde_json::Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
                    Some(other) => Err(Error::msg(format!("{} sets {name} to {other}; it takes text", g.name))),
                    None => Err(Error::msg(format!("{} has no {name}", g.name))),
                }
            };
            Ok(KeySource::Pkcs11 { library: g.base.join(field("library")?), token: field("token")?, key: field("key")? })
        }
    }
}

/// The settings file Pwrs.Signing reads.
fn settings_json(folder: &Path, module: &str, s: &Settings) -> serde_json::Value {
    let path = |p: &Path| p.display().to_string();
    let mut map = serde_json::Map::new();
    map.insert("folder".into(), path(folder).into());
    map.insert("module".into(), module.into());
    let (source, value): (&str, serde_json::Value) = match &s.source {
        KeySource::Pfx(p) => ("pfx", path(p).into()),
        KeySource::Pem { certificate, key } => ("pem", serde_json::json!([path(certificate), path(key)])),
        KeySource::Store(t) => ("store", t.as_str().into()),
        KeySource::Command { args, dir } => {
            map.insert("command_dir".into(), path(dir).into());
            ("command", serde_json::json!(args))
        }
        KeySource::Pkcs11 { library, token, key } => ("pkcs11", serde_json::json!({ "library": path(library), "token": token, "key": key })),
    };
    map.insert("source".into(), source.into());
    map.insert(source.into(), value);
    if let Some(e) = &s.password_env {
        map.insert("password_env".into(), e.as_str().into());
    }
    if let Some(c) = &s.certificate {
        map.insert("certificate".into(), path(c).into());
    }
    if let Some(u) = &s.timestamp_url {
        map.insert("timestamp_url".into(), u.as_str().into());
    }
    map.insert("hash".into(), s.hash.as_str().into());
    map.insert("catalog".into(), s.catalog.into());
    map.insert("signer".into(), if s.openauthenticode { "openauthenticode" } else { "pwrs" }.into());
    serde_json::Value::Object(map)
}

/// Signs the module folder `folder` with `settings`. The signer is
/// compiled under `work`, in a folder named for its source, the first
/// time a source is used.
pub fn sign(folder: &Path, settings: &Settings, work: &Path) -> Result<(), Error> {
    let (_, module) = crate::merge::manifest_of(folder)?;
    let tc = Toolchain::ensure()?;
    let texts: Vec<&str> = SIGNING_SOURCES.iter().map(|(_, t)| *t).chain([SIGN_PS1]).collect();
    let dir = work.join(format!("signing-{}", crate::generate::shell_stamp(&texts)));
    let assembly = dir.join("Pwrs.Signing.dll");
    if !assembly.is_file() {
        compile_signer(&tc, &dir)?;
    }
    let openauthenticode = if settings.openauthenticode { tc.openauthenticode()?.display().to_string() } else { String::new() };
    let script = pwsh::materialize_script(&dir, "sign.ps1", SIGN_PS1)?;
    let settings_file = dir.join(format!("settings-{}.json", std::process::id()));
    std::fs::write(&settings_file, settings_json(folder, &module, settings).to_string())
        .map_err(|e| Error::msg(format!("cannot write {}: {e}", settings_file.display())))?;
    let ran = pwsh::stream_pwsh_tool(&script, &[settings_file.display().to_string(), assembly.display().to_string(), openauthenticode]);
    if let Err(e) = std::fs::remove_file(&settings_file) {
        eprintln!("pwrs: cannot remove {}: {e}", settings_file.display());
    }
    ran.map_err(|e| Error::msg(format!("{module} is not signed: {e}")))
}

/// Compiles Pwrs.Signing into `dir`: into a folder of this process's own
/// beside it, renamed to `dir` when the compile succeeds. When another
/// run's folder is renamed there first, that one is used and this
/// process's folder is removed.
fn compile_signer(tc: &Toolchain, dir: &Path) -> Result<(), Error> {
    let pkcs = tc.pkcs()?;
    let mut staging = dir.as_os_str().to_os_string();
    staging.push(format!(".{}.partial", std::process::id()));
    let staging = PathBuf::from(staging);
    let sources = crate::build::write_sources(&staging.join("src"), SIGNING_SOURCES)?;
    let env = pwrs_build::CompileEnv { refs: &tc.net10_refs, defines: toolchain::CORE_DEFINES, work: &staging.join("rsp"), debug: DebugSymbols::Omit, path_map: &[] };
    let compiled = tc.compile(&env, &sources, &[pkcs], &staging.join("Pwrs.Signing.dll"));
    if let Err(e) = compiled {
        if let Err(left) = std::fs::remove_dir_all(&staging) {
            eprintln!("pwrs: cannot remove {}: {left}", staging.display());
        }
        return Err(e);
    }
    match std::fs::rename(&staging, dir) {
        Ok(()) => Ok(()),
        Err(_renamed_by_another_run) if dir.join("Pwrs.Signing.dll").is_file() => {
            if let Err(left) = std::fs::remove_dir_all(&staging) {
                eprintln!("pwrs: cannot remove {}: {left}", staging.display());
            }
            Ok(())
        }
        Err(e) => Err(Error::msg(format!("cannot move {} to {}: {e}", staging.display(), dir.display()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(text: &str) -> serde_json::Value {
        serde_json::from_str(text).expect("json")
    }

    fn read(table: &str, flags: &[(&str, Option<&str>)]) -> Result<Option<Settings>, Error> {
        let flags: Vec<(String, serde_json::Value)> =
            flags.iter().map(|(k, v)| (k.to_string(), flag_value(k, *v).expect("a flag's value"))).collect();
        settings(&json(table), Path::new("crate"), &flags, Path::new("here"))
    }

    fn refused(table: &str, flags: &[(&str, Option<&str>)], says: &str) {
        match read(table, flags) {
            Ok(s) => panic!("{table} {flags:?} was taken: {s:?}"),
            Err(e) => assert!(e.to_string().contains(says), "{table} {flags:?}: {e}"),
        }
    }

    #[test]
    fn each_key_source_reads_with_its_paths_joined_where_it_was_given() {
        assert_eq!(read("null", &[]).expect("nothing"), None);
        let pfx = read(r#"{ "pfx": "keys/signer.pfx", "password-env": "PFX_PASSWORD" }"#, &[]).expect("pfx").expect("settings");
        assert_eq!(pfx.source, KeySource::Pfx(Path::new("crate").join("keys/signer.pfx")));
        assert_eq!((pfx.password_env.as_deref(), pfx.hash.as_str(), pfx.catalog, pfx.openauthenticode), (Some("PFX_PASSWORD"), "sha256", false, false));
        let pem = read("null", &[("pem", Some(r#"["signer.pem", "signer.key"]"#))]).expect("pem").expect("settings");
        assert_eq!(pem.source, KeySource::Pem { certificate: Path::new("here").join("signer.pem"), key: Path::new("here").join("signer.key") });
        let store = read(r#"{ "store": "ab cd ef 01 23 45 67 89 ab cd ef 01 23 45 67 89 ab cd ef 01" }"#, &[]).expect("store").expect("settings");
        assert_eq!(store.source, KeySource::Store(String::from("ABCDEF0123456789ABCDEF0123456789ABCDEF01")));
        let command = read(r#"{ "command": ["sign-tool", "--in", "{digest}", "--out", "{signature}"], "certificate": "chain.pem", "hash": "sha384" }"#, &[]).expect("command").expect("settings");
        assert_eq!(
            command.source,
            KeySource::Command { args: vec!["sign-tool".into(), "--in".into(), "{digest}".into(), "--out".into(), "{signature}".into()], dir: PathBuf::from("crate") }
        );
        assert_eq!((command.certificate, command.hash.as_str()), (Some(Path::new("crate").join("chain.pem")), "sha384"));
        let flagged = read("null", &[("command", Some(r#"["signing/sign.sh", "{digest}"]"#)), ("certificate", Some("chain.pem"))]).expect("command flag").expect("settings");
        assert_eq!(flagged.source, KeySource::Command { args: vec!["signing/sign.sh".into(), "{digest}".into()], dir: PathBuf::from("here") });
        let token = read(r#"{ "pkcs11": { "library": "softhsm2.dll", "token": "signing", "key": "code" }, "certificate": "chain.pem", "password-env": "PIN" }"#, &[("catalog", None), ("timestamp-url", Some("http://127.0.0.1:8080/"))])
            .expect("pkcs11")
            .expect("settings");
        assert_eq!(token.source, KeySource::Pkcs11 { library: Path::new("crate").join("softhsm2.dll"), token: "signing".into(), key: "code".into() });
        assert_eq!((token.catalog, token.timestamp_url.as_deref()), (true, Some("http://127.0.0.1:8080/")));
    }

    #[test]
    fn a_flag_replaces_its_setting_and_a_key_flag_replaces_the_tables_key() {
        let s = read(r#"{ "pfx": "signer.pfx", "hash": "sha512", "signer": "openauthenticode" }"#, &[("store", Some("0123456789abcdef0123456789abcdef01234567")), ("hash", Some("sha384"))])
            .expect("settings")
            .expect("some");
        assert_eq!(s.source, KeySource::Store(String::from("0123456789ABCDEF0123456789ABCDEF01234567")));
        assert_eq!((s.hash.as_str(), s.openauthenticode), ("sha384", true));
    }

    #[test]
    fn every_setting_out_of_shape_is_refused_naming_it() {
        refused(r#"{ "pfx": "a.pfx", "store": "0123456789abcdef0123456789abcdef01234567" }"#, &[], "name 2 keys, pfx and store; give one");
        refused(r#"{ "hash": "sha256" }"#, &[], "name no key: give one of pfx, pem, store, command, pkcs11");
        refused(r#"{ "pfx": "a.pfx", "key": "x" }"#, &[], "[package.metadata.pwrs.signing] sets `key`, which is not one of");
        refused(r#"{ "pem": ["only.pem"] }"#, &[], "takes two files, the certificates and the key");
        refused(r#"{ "store": "not-a-thumbprint" }"#, &[], "40 hexadecimal digits");
        refused(r#"{ "command": ["sign-tool", "--out", "{signature}"], "certificate": "c.pem" }"#, &[], "names none of {digest}, {digest-base64}, {digest-hex}");
        refused(r#"{ "command": ["sign-tool", "{digest}"] }"#, &[], "a command key needs certificate");
        refused(r#"{ "pfx": "a.pfx", "certificate": "c.pem" }"#, &[], "a pfx key comes with its own");
        refused(r#"{ "pkcs11": { "library": "p.so", "token": "t" }, "certificate": "c.pem" }"#, &[], "[package.metadata.pwrs.signing] pkcs11 has no key");
        refused(r#"{ "pkcs11": { "library": "p.so", "token": "t", "key": "k", "slot": 0 }, "certificate": "c.pem" }"#, &[], "sets `slot`, which is not one of library, token, key");
        refused(r#"{ "pfx": "a.pfx", "password-env": "hunter2=yes" }"#, &[], "give the name of the environment variable set to the secret");
        refused(r#"{ "pfx": "a.pfx", "timestamp-url": "timestamp.example" }"#, &[], "the http or https address of an RFC 3161 timestamp server");
        refused(r#"{ "pfx": "a.pfx", "hash": "sha1" }"#, &[], "it takes sha256, sha384 or sha512");
        refused(r#"{ "pfx": "a.pfx", "catalog": "yes" }"#, &[], "it takes true or false");
        refused(r#"{ "pfx": "a.pfx", "signer": "signtool" }"#, &[], r#"it takes "pwrs" or "openauthenticode""#);
        refused(r#"{ "command": ["sign-tool", "{digest}"], "certificate": "c.pem", "signer": "openauthenticode" }"#, &[], "OpenAuthenticode takes only a certificate that has its key");
        refused(r#"["pfx"]"#, &[], "it is a table of the signing settings");
    }

    #[test]
    fn a_flag_takes_text_or_its_value_as_cargo_toml_writes_it() {
        assert_eq!(flag_value("catalog", None).expect("catalog"), serde_json::Value::Bool(true));
        assert!(flag_value("catalog", Some("false")).expect_err("a value after --catalog").to_string().contains("--catalog takes no value"));
        assert_eq!(flag_value("pfx", Some("signer.pfx")).expect("pfx"), json(r#""signer.pfx""#));
        assert_eq!(flag_value("pem", Some(r#"["a.pem", "a.key"]"#)).expect("pem"), json(r#"["a.pem", "a.key"]"#));
        assert_eq!(flag_value("pkcs11", Some(r#"{ library = "p.so", token = "t", key = "k" }"#)).expect("pkcs11"), json(r#"{ "library": "p.so", "token": "t", "key": "k" }"#));
        let e = flag_value("command", Some("sign-tool {digest}")).expect_err("not a TOML value");
        assert!(e.to_string().contains("--command takes its value as Cargo.toml writes it"), "{e}");
        assert!(flag_value("hash", None).expect_err("no value").to_string().contains("--hash takes a value"));
    }

    #[test]
    fn the_settings_file_names_the_source_and_every_setting_given() {
        let s = Settings {
            source: KeySource::Pkcs11 { library: PathBuf::from("p.so"), token: "t".into(), key: "k".into() },
            password_env: Some("PIN".into()),
            certificate: Some(PathBuf::from("c.pem")),
            timestamp_url: None,
            hash: "sha256".into(),
            catalog: true,
            openauthenticode: false,
        };
        let v = settings_json(Path::new("Demo"), "Demo", &s);
        assert_eq!(v["source"], "pkcs11");
        assert_eq!(v["pkcs11"]["token"], "t");
        assert_eq!((v["password_env"].as_str(), v["certificate"].as_str(), v["catalog"].as_bool(), v["signer"].as_str()), (Some("PIN"), Some("c.pem"), Some(true), Some("pwrs")));
        assert!(v.get("timestamp_url").is_none());
        assert!(v.get("command_dir").is_none());
        let command = Settings { source: KeySource::Command { args: vec!["sign.sh".into(), "{digest}".into()], dir: PathBuf::from("crate") }, ..s };
        let v = settings_json(Path::new("Demo"), "Demo", &command);
        assert_eq!((v["source"].as_str(), v["command"][0].as_str(), v["command_dir"].as_str()), (Some("command"), Some("sign.sh"), Some("crate")));
    }
}
