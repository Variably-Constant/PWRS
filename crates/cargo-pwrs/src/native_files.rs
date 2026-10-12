//! Files a module ships beside its native library, from
//! `[package.metadata.pwrs.native-files]`: each downloaded once with curl
//! into `<CARGO_HOME>/pwrs/downloads/<sha256>/`, checked against the
//! SHA-256 its entry pins, taken from the zip or gzip-compressed tar
//! archive the download is, or used as downloaded, and written to
//! `runtimes/<rid>/native` with the license texts its notices quote.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::Error;

/// A file a module ships beside its native library.
#[derive(Debug, Clone)]
pub struct NativeFile {
    /// The file's name in `runtimes/<rid>/native`, the entry's key.
    pub name: String,
    /// Where it is downloaded from: any address curl takes.
    pub url: String,
    /// The download's SHA-256, in lower-case hexadecimal.
    pub sha256: String,
    /// The file's path inside the archive the download is, with `/`
    /// between its parts; `None` when the download is the file itself.
    pub path: Option<String>,
    /// License files inside the archive, by path, with `/` between parts.
    pub licenses: Vec<String>,
    /// License files in the package's folder: as written, and resolved.
    pub license_files: Vec<(String, PathBuf)>,
    /// Where the entry is written, as refusals name it.
    pub source: String,
    /// The expression of the `cfg(...)` table the entry is written in,
    /// which the build's target must satisfy for the entry to apply;
    /// `None` for an entry of the unscoped table.
    pub scope: Option<crate::cfg::Expr>,
}

/// A license text a shipped file carries: the file it was read from, as
/// the entry writes it, and its text.
#[derive(Debug)]
pub struct License {
    pub written: String,
    pub text: String,
}

/// A file written beside the library, with its license texts.
#[derive(Debug)]
pub struct Shipped {
    pub file: NativeFile,
    pub licenses: Vec<License>,
}

/// The keys an entry's table takes.
const KEYS: [&str; 5] = ["url", "sha256", "path", "licenses", "license-files"];

/// The entries of a `native-files` table: each key the name of a file,
/// each value a table of `url`, `sha256`, and `path`, `licenses` and
/// `license-files`; and beside them tables under `cfg(...)` keys, as
/// `license-files` scopes its own, whose entries take the same form.
/// `source` names the table in refusals. An absent table is no entries.
pub fn from_metadata(table: &serde_json::Value, package_dir: &Path, source: &str) -> Result<Vec<NativeFile>, Error> {
    if table.is_null() {
        return Ok(Vec::new());
    }
    let map = table.as_object().ok_or_else(|| Error::msg(format!("{source} is a table of file names, each with its url and sha256")))?;
    let mut out = Vec::with_capacity(map.len());
    for (key, value) in map {
        let Some(text) = crate::cfg::key_expression(key) else {
            out.push(entry(key, value, package_dir, source, None)?);
            continue;
        };
        let expr = crate::cfg::parse(text).map_err(|why| Error::msg(format!("{source} key `{key}` is not a cfg expression: {why}")))?;
        if let Some(name) = crate::cfg::non_target_name(&expr) {
            return Err(Error::msg(format!(
                "{source} key `{key}` names `{name}`, which describes the compilation rather than the target, and a table applies by target alone"
            )));
        }
        let entries = value.as_object().ok_or_else(|| Error::msg(format!("{source} key `{key}` is not a table of file names")))?;
        let scoped = format!("{source}.'{key}'");
        for (name, value) in entries {
            out.push(entry(name, value, package_dir, &scoped, Some(&expr))?);
        }
    }
    Ok(out)
}

/// One entry of a `native-files` table, or of one of its `cfg(...)`
/// tables when `scope` is that table's expression.
fn entry(name: &str, value: &serde_json::Value, package_dir: &Path, source: &str, scope: Option<&crate::cfg::Expr>) -> Result<NativeFile, Error> {
    if let Some(why) = name_refusal(name) {
        return Err(Error::msg(format!("{source} key `{name}` {why}")));
    }
    let table = value.as_object().ok_or_else(|| Error::msg(format!("{source} `{name}` is not a table; it takes {}", KEYS.join(", "))))?;
    if let Some(other) = table.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(Error::msg(format!("{source} `{name}` sets `{other}`; an entry takes {}", KEYS.join(", "))));
    }
    let text = |key: &str| -> Result<Option<String>, Error> {
        match table.get(key) {
            None => Ok(None),
            Some(serde_json::Value::String(s)) if !s.trim().is_empty() => Ok(Some(s.clone())),
            Some(other) => Err(Error::msg(format!("{source} `{name}` sets `{key}` to {other}; it takes text"))),
        }
    };
    let list = |key: &str| -> Result<Vec<String>, Error> {
        let Some(value) = table.get(key) else {
            return Ok(Vec::new());
        };
        let not_a_list = || Error::msg(format!("{source} `{name}` sets `{key}` to {value}; it takes a list of paths"));
        let mut out: Vec<String> = Vec::new();
        for item in value.as_array().ok_or_else(not_a_list)? {
            let written = match item.as_str() {
                Some(s) if !s.trim().is_empty() => s.replace('\\', "/"),
                _not_a_path => return Err(not_a_list()),
            };
            if out.contains(&written) {
                return Err(Error::msg(format!("{source} `{name}` lists {written} twice in `{key}`")));
            }
            out.push(written);
        }
        Ok(out)
    };
    let url = text("url")?.ok_or_else(|| Error::msg(format!("{source} `{name}` has no `url`, the address it is downloaded from")))?;
    let sha256 = text("sha256")?
        .ok_or_else(|| Error::msg(format!("{source} `{name}` has no `sha256`, the hash its download must have")))?
        .to_ascii_lowercase();
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::msg(format!("{source} `{name}` sets sha256 to {sha256}, which is not 64 hexadecimal digits")));
    }
    let path = text("path")?.map(|p| p.replace('\\', "/"));
    let licenses = list("licenses")?;
    if path.is_none() && !licenses.is_empty() {
        return Err(Error::msg(format!(
            "{source} `{name}` lists `licenses`, which are read from inside an archive, and gives no `path` into one, so its download is the file itself; list its license files in the package's folder under `license-files`"
        )));
    }
    let license_files: Vec<(String, PathBuf)> = list("license-files")?
        .into_iter()
        .map(|written| {
            let resolved = package_dir.join(&written);
            (written, resolved)
        })
        .collect();
    if licenses.is_empty() && license_files.is_empty() {
        return Err(Error::msg(format!(
            "{source} `{name}` lists no license file; the notices beside the library quote each shipped file's licenses, so list them under `licenses`, inside its archive, or `license-files`, in the package's folder"
        )));
    }
    Ok(NativeFile { name: name.to_string(), url, sha256, path, licenses, license_files, source: source.to_string(), scope: scope.cloned() })
}

/// Why `name` cannot be a file shipped beside the library, or `None` when
/// it can: a plain file name that Windows, macOS and Linux all take, and
/// not the notices file's.
fn name_refusal(name: &str) -> Option<String> {
    if name.is_empty() || name == "." || name == ".." {
        return Some(String::from("is not a file name"));
    }
    if let Some(c) = name.chars().find(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()) {
        return Some(format!("holds {c:?}, which a file name on Windows cannot"));
    }
    if name.ends_with(['.', ' ']) {
        return Some(String::from("ends with a period or a space, which Windows drops from a file name"));
    }
    if name.eq_ignore_ascii_case(crate::notices::FILE_NAME) {
        return Some(format!("is {}, the notices file's own name", crate::notices::FILE_NAME));
    }
    None
}

/// The entries that apply to a target whose cfg is `target`: the unscoped
/// table's, and each `cfg(...)` table's whose expression the target
/// satisfies. Two of them naming one file, without regard to case, as
/// Windows and macOS compare names, are refused.
pub fn applying<'a>(files: &'a [NativeFile], target: &[crate::cfg::Cfg]) -> Result<Vec<&'a NativeFile>, Error> {
    let mut out: Vec<&NativeFile> = Vec::new();
    for file in files.iter().filter(|f| f.scope.as_ref().is_none_or(|e| e.matches(target))) {
        if let Some(first) = out.iter().find(|o| o.name.eq_ignore_ascii_case(&file.name)) {
            return Err(Error::msg(format!(
                "{} `{}` and {} `{}` are one file for this target; keep one of them",
                first.source, first.name, file.source, file.name
            )));
        }
        out.push(file);
    }
    Ok(out)
}

/// The folder downloads are kept in, `<CARGO_HOME>/pwrs/downloads`, with
/// CARGO_HOME as cargo reads it: the variable, or `.cargo` in the home
/// folder when it is unset.
pub fn downloads_dir() -> Result<PathBuf, Error> {
    let home = match std::env::var_os("CARGO_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _unset_or_empty => match std::env::home_dir() {
            Some(home) => home.join(".cargo"),
            None => return Err(Error::msg("CARGO_HOME is unset and the home folder is unknown, so there is no folder to keep downloads in")),
        },
    };
    Ok(home.join("pwrs").join("downloads"))
}

/// The name a download is kept under: the last part of its address's
/// path, of letters, digits, `.`, `-` and `_` alone, or `download` when
/// nothing of it is left.
fn download_name(url: &str) -> String {
    let path = match url.find(['?', '#']) {
        Some(at) => &url[..at],
        None => url,
    };
    let last = match path.rfind('/') {
        Some(at) => &path[at + 1..],
        None => path,
    };
    let kept: String = last.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).collect();
    if kept.chars().all(|c| c == '.') { String::from("download") } else { kept }
}

/// The SHA-256 of the file at `path`, in lower-case hexadecimal.
fn sha256_of(path: &Path) -> Result<String, Error> {
    let mut file = File::open(path).map_err(|e| Error::msg(format!("cannot open {}: {e}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buffer).map_err(|e| Error::msg(format!("cannot read {}: {e}", path.display())))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Removes the partial download at `path`, answering what to add to an
/// error's message when it cannot be removed.
fn discard(path: &Path) -> String {
    match std::fs::remove_file(path) {
        Ok(()) => String::new(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => format!("; {} could not be removed: {e}", path.display()),
    }
}

/// The download `file` names: the copy kept under `downloads` while it
/// still has the pinned SHA-256, or else one fetched with curl, checked,
/// and kept there for later builds. A download with any other SHA-256 is
/// refused, naming both, and nothing of it is kept.
pub fn fetch(file: &NativeFile, downloads: &Path) -> Result<PathBuf, Error> {
    let folder = downloads.join(&file.sha256);
    let name = download_name(&file.url);
    let kept = folder.join(&name);
    if kept.is_file() {
        if sha256_of(&kept)? == file.sha256 {
            return Ok(kept);
        }
        std::fs::remove_file(&kept)
            .map_err(|e| Error::msg(format!("{} no longer has the SHA-256 it was kept under, and cannot be removed: {e}", kept.display())))?;
        eprintln!("pwrs: {} no longer had the SHA-256 it was kept under; fetching it again", kept.display());
    }
    std::fs::create_dir_all(&folder).map_err(|e| Error::msg(format!("cannot create {}: {e}", folder.display())))?;
    let partial = folder.join(format!("{name}.partial-{}", std::process::id()));
    eprintln!("pwrs: downloading {} for {}", file.url, file.name);
    let mut curl = Command::new("curl");
    curl.args(["--fail", "--location", "--silent", "--show-error", "--retry", "3", "--output"]).arg(&partial).arg(&file.url);
    let out = pwrs_build::trace::output(&mut curl)
        .map_err(|e| Error::msg(format!("cannot run curl, which cargo-pwrs downloads native files with: {e}. Install curl, or put it on PATH")))?;
    if !out.status.success() {
        return Err(Error::msg(format!(
            "{} `{}`: curl could not download {}: {}{}",
            file.source,
            file.name,
            file.url,
            String::from_utf8_lossy(&out.stderr).trim(),
            discard(&partial)
        )));
    }
    let got = sha256_of(&partial)?;
    if got != file.sha256 {
        return Err(Error::msg(format!(
            "{} `{}`: {} has SHA-256 {got}, and the entry pins {}; nothing is taken from it{}",
            file.source,
            file.name,
            file.url,
            file.sha256,
            discard(&partial)
        )));
    }
    match std::fs::rename(&partial, &kept) {
        Ok(()) => Ok(kept),
        Err(e) if kept.is_file() => {
            eprintln!("pwrs: another build kept {} first ({e}){}", kept.display(), discard(&partial));
            Ok(kept)
        }
        Err(e) => Err(Error::msg(format!("cannot keep {} as {}: {e}{}", partial.display(), kept.display(), discard(&partial)))),
    }
}

/// The kinds of archive a download can be, told by its first bytes.
enum Kind {
    Zip,
    TarGz,
}

/// The kind of archive the file at `path` is, or `None` for neither.
fn kind_of(path: &Path) -> Result<Option<Kind>, Error> {
    let mut file = File::open(path).map_err(|e| Error::msg(format!("cannot open {}: {e}", path.display())))?;
    let mut head = [0u8; 4];
    let mut read = 0;
    while read < head.len() {
        let n = file.read(&mut head[read..]).map_err(|e| Error::msg(format!("cannot read {}: {e}", path.display())))?;
        if n == 0 {
            break;
        }
        read += n;
    }
    Ok(match &head[..read] {
        [b'P', b'K', 3 | 5, 4 | 6] => Some(Kind::Zip),
        [0x1f, 0x8b, ..] => Some(Kind::TarGz),
        _neither => None,
    })
}

/// A license file's text, refused when it holds nothing.
fn license_text(file: &NativeFile, written: &str, bytes: &[u8]) -> Result<String, Error> {
    let text = String::from_utf8_lossy(bytes).into_owned();
    if text.trim().is_empty() {
        return Err(Error::msg(format!("{} `{}`: the license file {written} is empty", file.source, file.name)));
    }
    Ok(text)
}

/// Writes `bytes` to `dest`, executable on Unix when the archive marks the
/// entry so.
#[cfg_attr(not(unix), allow(unused_variables))]
fn write_file(dest: &Path, bytes: &[u8], executable: bool) -> Result<(), Error> {
    std::fs::write(dest, bytes).map_err(|e| Error::msg(format!("cannot write {}: {e}", dest.display())))?;
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755)).map_err(|e| Error::msg(format!("cannot make {} executable: {e}", dest.display())))?;
    }
    Ok(())
}

/// An entry's path as the archive names it, with `/` between its parts and
/// no leading `./`.
fn entry_path(name: &str) -> String {
    let name = name.replace('\\', "/");
    match name.strip_prefix("./") {
        Some(rest) => rest.to_string(),
        None => name,
    }
}

/// Writes the entry `inner` of the archive `download` to `dest`, and
/// answers the texts of the license files `file` lists inside it, in the
/// order it lists them. An archive missing either is refused, naming what
/// it lacks.
fn from_archive(file: &NativeFile, inner: &str, download: &Path, dest: &Path) -> Result<Vec<License>, Error> {
    let at = |what: String| Error::msg(format!("{} `{}`: {what}", file.source, file.name));
    let open = || File::open(download).map_err(|e| at(format!("cannot open {}: {e}", download.display())));
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    let mut found = false;
    let mut keep = |name: &str, bytes: Vec<u8>, executable: bool| -> Result<(), Error> {
        if name == inner {
            write_file(dest, &bytes, executable)?;
            found = true;
        }
        if file.licenses.iter().any(|l| l == name) {
            texts.insert(name.to_string(), license_text(file, name, &bytes)?);
        }
        Ok(())
    };
    let wanted = |name: &str| name == inner || file.licenses.iter().any(|l| l == name);
    match kind_of(download)? {
        Some(Kind::Zip) => {
            let mut archive = zip::ZipArchive::new(open()?).map_err(|e| at(format!("{} is not a zip archive this can read: {e}", file.url)))?;
            for index in 0..archive.len() {
                let mut entry = archive.by_index(index).map_err(|e| at(format!("cannot read entry {index} of {}: {e}", file.url)))?;
                let name = entry_path(&entry.name().map_err(|e| at(format!("cannot read the name of entry {index} of {}: {e}", file.url)))?);
                if !wanted(&name) {
                    continue;
                }
                let executable = entry.unix_mode().is_some_and(|mode| mode & 0o111 != 0);
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).map_err(|e| at(format!("cannot read {name} in {}: {e}", file.url)))?;
                keep(&name, bytes, executable)?;
            }
        }
        Some(Kind::TarGz) => {
            let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(open()?));
            let entries = archive.entries().map_err(|e| at(format!("{} is not a gzip-compressed tar archive this can read: {e}", file.url)))?;
            for entry in entries {
                let mut entry = entry.map_err(|e| at(format!("cannot read an entry of {}: {e}", file.url)))?;
                let name = entry_path(&entry.path().map_err(|e| at(format!("cannot read an entry's path in {}: {e}", file.url)))?.to_string_lossy());
                if !wanted(&name) {
                    continue;
                }
                let mode = entry.header().mode().map_err(|e| at(format!("cannot read the mode of {name} in {}: {e}", file.url)))?;
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).map_err(|e| at(format!("cannot read {name} in {}: {e}", file.url)))?;
                keep(&name, bytes, mode & 0o111 != 0)?;
            }
        }
        None => {
            return Err(at(format!(
                "{} is neither a zip archive nor a gzip-compressed tar one, and the entry names a `path` inside an archive",
                file.url
            )));
        }
    }
    if !found {
        return Err(at(format!("{} holds no {inner}", file.url)));
    }
    let mut out = Vec::with_capacity(file.licenses.len());
    for written in &file.licenses {
        match texts.remove(written) {
            Some(text) => out.push(License { written: written.clone(), text }),
            None => return Err(at(format!("{} holds no license file {written}", file.url))),
        }
    }
    Ok(out)
}

/// Writes `file` to `dest` from `download`: the entry `path` names inside
/// the zip or gzip-compressed tar archive the download is, or the download
/// itself when the entry names none. Answers its license texts, those
/// `licenses` names inside the archive and then those `license-files`
/// names in the package's folder, each there and not empty.
pub fn take(file: &NativeFile, download: &Path, dest: &Path) -> Result<Vec<License>, Error> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::msg(format!("cannot create {}: {e}", parent.display())))?;
    }
    let mut licenses = match &file.path {
        Some(inner) => from_archive(file, inner, download, dest)?,
        None => {
            std::fs::copy(download, dest).map_err(|e| Error::msg(format!("cannot copy {} to {}: {e}", download.display(), dest.display())))?;
            Vec::new()
        }
    };
    for (written, path) in &file.license_files {
        let bytes = std::fs::read(path)
            .map_err(|e| Error::msg(format!("{} `{}` license-files: cannot read {}: {e}", file.source, file.name, path.display())))?;
        licenses.push(License { written: written.clone(), text: license_text(file, written, &bytes)? });
    }
    Ok(licenses)
}

/// Writes each file of `files` that applies to a target whose cfg is
/// `target` into `native_dir`, through the downloads kept under
/// `downloads`, and answers them with their license texts. A file the
/// folder already holds, the library or a helper, is never replaced.
pub fn ship(files: &[NativeFile], target: &[crate::cfg::Cfg], native_dir: &Path, downloads: &Path) -> Result<Vec<Shipped>, Error> {
    let applying = applying(files, target)?;
    if applying.is_empty() {
        return Ok(Vec::new());
    }
    let mut held: Vec<String> = Vec::new();
    let entries = std::fs::read_dir(native_dir).map_err(|e| Error::msg(format!("cannot read {}: {e}", native_dir.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::msg(format!("cannot read an entry of {}: {e}", native_dir.display())))?;
        held.push(entry.file_name().to_string_lossy().into_owned());
    }
    let mut out = Vec::with_capacity(applying.len());
    for file in applying {
        if let Some(there) = held.iter().find(|h| h.eq_ignore_ascii_case(&file.name)) {
            return Err(Error::msg(format!("{} `{}` would replace {there}, which the build ships in the same folder", file.source, file.name)));
        }
        let download = fetch(file, downloads)?;
        let licenses = take(file, &download, &native_dir.join(&file.name))?;
        out.push(Shipped { file: file.clone(), licenses });
    }
    Ok(out)
}

/// The notices of the files shipped beside `library`, which follow the
/// library's own: each file with the address it was downloaded from, the
/// SHA-256 it was checked against and its place in the archive, then its
/// license texts. Empty when none ship.
pub fn notices(shipped: &[Shipped], library: &str) -> String {
    use crate::notices::{RULE, THIN};
    if shipped.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "\n{RULE}\nFiles shipped beside {library}\n\
         Each was downloaded from the address given, checked against the SHA-256\n\
         given, and is quoted below with its license files.\n"
    );
    for s in shipped {
        out.push_str(&format!("\n{RULE}\n{}\nFrom: {}\nSHA-256: {}\n", s.file.name, s.file.url, s.file.sha256));
        if let Some(path) = &s.file.path {
            out.push_str(&format!("Taken from: {path}\n"));
        }
        for license in &s.licenses {
            out.push_str(&format!("{THIN}\n{}\n{THIN}\n{}\n", license.written, license.text.trim_end()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn table(json: &str) -> serde_json::Value {
        serde_json::from_str(json).expect("json")
    }

    const SOURCE: &str = "[package.metadata.pwrs] native-files";

    const PIN: &str = "ad31b1fc8443175d204f77a611fdb3ef2ec42759bdc2f1167368de24a4a7e7f1";

    fn parse(json: &str) -> Result<Vec<NativeFile>, Error> {
        from_metadata(&table(json), Path::new("pkg"), SOURCE)
    }

    fn refused(json: &str, why: &str) {
        match parse(json) {
            Ok(files) => panic!("{json} was taken as {files:?}"),
            Err(e) => assert!(e.to_string().contains(why), "{json}: {e}"),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pwrs-native-files-{name}-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("clear the scratch folder");
        }
        std::fs::create_dir_all(&dir).expect("make the scratch folder");
        dir
    }

    /// The address curl reads a local file at.
    fn file_url(path: &Path) -> String {
        let written = path.display().to_string().replace('\\', "/").replace(' ', "%20");
        if written.starts_with('/') { format!("file://{written}") } else { format!("file:///{written}") }
    }

    fn sha256_text(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    }

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).unix_permissions(0o755);
            writer.start_file(*name, options).expect("start a zip entry");
            writer.write_all(bytes).expect("write a zip entry");
        }
        writer.finish().expect("finish the zip").into_inner()
    }

    fn tar_gz_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default()));
        for (name, bytes) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, name, *bytes).expect("append a tar entry");
        }
        builder.into_inner().expect("finish the tar").finish().expect("finish the gzip stream")
    }

    #[test]
    fn an_entry_takes_an_address_a_pin_and_its_licenses_scoped_as_license_files_are() {
        let files = parse(&format!(
            r#"{{
                "plain.bin": {{ "url": "https://example.org/plain.bin", "sha256": "{}", "license-files": ["licenses/plain.txt"] }},
                "cfg(all(windows, target_arch = \"aarch64\"))": {{
                    "dxcompiler.dll": {{ "url": "https://example.org/dxc.zip", "sha256": "{PIN}", "path": "bin\\arm64\\dxcompiler.dll", "licenses": ["LICENSE-MS.txt", "LICENSE-LLVM.txt"] }}
                }}
            }}"#,
            PIN.to_ascii_uppercase()
        ))
        .expect("a table of two entries");
        assert_eq!(files.len(), 2);
        let plain = files.iter().find(|f| f.name == "plain.bin").expect("the plain entry");
        assert_eq!(plain.sha256, PIN, "the pin is held in lower case");
        assert_eq!(plain.path, None);
        assert_eq!(plain.license_files, vec![(String::from("licenses/plain.txt"), Path::new("pkg").join("licenses/plain.txt"))]);
        assert!(plain.scope.is_none());
        let dxc = files.iter().find(|f| f.name == "dxcompiler.dll").expect("the scoped entry");
        assert_eq!(dxc.path.as_deref(), Some("bin/arm64/dxcompiler.dll"), "a path is held with forward slashes");
        assert_eq!(dxc.licenses, ["LICENSE-MS.txt", "LICENSE-LLVM.txt"]);
        assert_eq!(dxc.source, format!("{SOURCE}.'cfg(all(windows, target_arch = \"aarch64\"))'"));
        let arm = crate::cfg::target("target_family=\"windows\"\ntarget_os=\"windows\"\ntarget_arch=\"aarch64\"\nwindows\n").expect("an arm64 cfg");
        let x64 = crate::cfg::target("target_family=\"windows\"\ntarget_os=\"windows\"\ntarget_arch=\"x86_64\"\nwindows\n").expect("an x64 cfg");
        let names = |cfg: &[crate::cfg::Cfg]| -> Vec<String> { applying(&files, cfg).expect("no two name one file").iter().map(|f| f.name.clone()).collect() };
        assert_eq!(names(&arm), ["dxcompiler.dll", "plain.bin"], "entries come in the order of their keys");
        assert_eq!(names(&x64), ["plain.bin"]);
        assert!(parse("null").expect("an absent table").is_empty());
    }

    #[test]
    fn entries_that_cannot_ship_are_refused_naming_why() {
        let entry = |name: &str, body: &str| format!(r#"{{ "{name}": {{ {body} }} }}"#);
        let good = format!(r#""url": "https://example.org/a.zip", "sha256": "{PIN}", "path": "a.dll", "licenses": ["LICENSE"]"#);
        refused(&entry("bin/a.dll", &good), "holds '/'");
        refused(&entry("..", &good), "is not a file name");
        refused(&entry("a.dll.", &good), "ends with a period");
        refused(&entry("third-party-notices.txt", &good), "the notices file's own name");
        refused(&entry("a.dll", &format!("{good}, \"mirror\": \"x\"")), "sets `mirror`");
        refused(&entry("a.dll", &format!(r#""sha256": "{PIN}", "path": "a.dll", "licenses": ["LICENSE"]"#)), "has no `url`");
        refused(&entry("a.dll", r#""url": "https://example.org/a.zip", "sha256": "abc", "path": "a.dll", "licenses": ["LICENSE"]"#), "is not 64 hexadecimal digits");
        refused(&entry("a.dll", &format!(r#""url": "https://example.org/a.dll", "sha256": "{PIN}", "licenses": ["LICENSE"]"#)), "under `license-files`");
        refused(&entry("a.dll", &format!(r#""url": "https://example.org/a.zip", "sha256": "{PIN}", "path": "a.dll""#)), "lists no license file");
        refused(&entry("a.dll", &format!(r#""url": "https://example.org/a.zip", "sha256": "{PIN}", "path": "a.dll", "licenses": ["L", "L"]"#)), "lists L twice");
        refused(&entry("a.dll", &format!(r#""url": 3, "sha256": "{PIN}", "path": "a.dll", "licenses": ["L"]"#)), "sets `url` to 3; it takes text");
        refused(r#"{ "a.dll": "https://example.org/a.dll" }"#, "is not a table");
        refused(&format!(r#"{{ "cfg(feature = \"gpu\")": {{ "a.dll": {{ {good} }} }} }}"#), "describes the compilation");
        refused(&format!(r#"{{ "cfg(target_os = linux)": {{ "a.dll": {{ {good} }} }} }}"#), "is not a cfg expression");
        let twice = parse(&format!(r#"{{ "A.dll": {{ {good} }}, "cfg(windows)": {{ "a.dll": {{ {good} }} }} }}"#)).expect("each entry is well formed");
        let windows = crate::cfg::target("target_family=\"windows\"\ntarget_os=\"windows\"\nwindows\n").expect("a windows cfg");
        let linux = crate::cfg::target("target_family=\"unix\"\ntarget_os=\"linux\"\nunix\n").expect("a linux cfg");
        let clash = applying(&twice, &windows).expect_err("two entries name one file on Windows");
        assert!(clash.to_string().contains("are one file for this target"), "{clash}");
        assert_eq!(applying(&twice, &linux).expect("one applies on Linux").len(), 1);
    }

    #[test]
    fn a_download_is_kept_under_the_last_part_of_its_address() {
        assert_eq!(download_name("https://github.com/o/r/releases/download/v1/dxc_2026_09_29.zip"), "dxc_2026_09_29.zip");
        assert_eq!(download_name("https://example.org/get?file=a.zip#top"), "get");
        assert_eq!(download_name("https://example.org/"), "download");
        assert_eq!(download_name("file:///C:/scratch/a%20b.tar.gz"), "a20b.tar.gz");
    }

    #[test]
    fn files_ship_from_a_zip_a_tar_gz_and_a_plain_download_and_later_builds_take_the_kept_copy() {
        let dir = scratch("ship");
        let served = dir.join("served");
        let package = dir.join("package");
        let native = dir.join("native");
        let downloads = dir.join("downloads");
        for folder in [&served, &package.join("licenses"), &native] {
            std::fs::create_dir_all(folder).expect("make a folder");
        }
        std::fs::write(native.join("demo.dll"), b"the library").expect("write the library");
        let zip = zip_of(&[("bin/arm64/dxcompiler.dll", b"dxc"), ("LICENSE-MS.txt", b"Microsoft's terms"), ("LICENSE-LLVM.txt", b"LLVM's terms")]);
        let tgz = tar_gz_of(&[("./lib/libtool.so", b"tool"), ("./COPYING", b"the tool's license")]);
        std::fs::write(served.join("dxc.zip"), &zip).expect("serve the zip");
        std::fs::write(served.join("tool.tar.gz"), &tgz).expect("serve the tar.gz");
        std::fs::write(served.join("plain.bin"), b"plain bytes").expect("serve the plain file");
        std::fs::write(package.join("licenses").join("plain.txt"), b"the plain file's license").expect("write a license file");
        let json = format!(
            r#"{{
                "dxcompiler.dll": {{ "url": "{}", "sha256": "{}", "path": "bin/arm64/dxcompiler.dll", "licenses": ["LICENSE-MS.txt", "LICENSE-LLVM.txt"] }},
                "libtool.so": {{ "url": "{}", "sha256": "{}", "path": "lib/libtool.so", "licenses": ["COPYING"] }},
                "plain.bin": {{ "url": "{}", "sha256": "{}", "license-files": ["licenses/plain.txt"] }}
            }}"#,
            file_url(&served.join("dxc.zip")),
            sha256_text(&zip),
            file_url(&served.join("tool.tar.gz")),
            sha256_text(&tgz),
            file_url(&served.join("plain.bin")),
            sha256_text(b"plain bytes"),
        );
        let files = from_metadata(&table(&json), &package, SOURCE).expect("three entries");
        let shipped = ship(&files, &[], &native, &downloads).expect("ship the three files");
        assert_eq!(std::fs::read(native.join("dxcompiler.dll")).expect("the dll"), b"dxc");
        assert_eq!(std::fs::read(native.join("libtool.so")).expect("the library from the tar.gz"), b"tool");
        assert_eq!(std::fs::read(native.join("plain.bin")).expect("the plain file"), b"plain bytes");
        let dxc = shipped.iter().find(|s| s.file.name == "dxcompiler.dll").expect("the dll shipped");
        let written: Vec<&str> = dxc.licenses.iter().map(|l| l.written.as_str()).collect();
        assert_eq!(written, ["LICENSE-MS.txt", "LICENSE-LLVM.txt"], "licenses come in the order the entry lists them");
        let text = notices(&shipped, "demo.dll");
        for quoted in ["Files shipped beside demo.dll", "Microsoft's terms", "LLVM's terms", "the tool's license", "the plain file's license", "Taken from: lib/libtool.so"] {
            assert!(text.contains(quoted), "the notices lack {quoted}:\n{text}");
        }
        assert!(downloads.join(sha256_text(&zip)).join("dxc.zip").is_file(), "the zip is kept under its SHA-256");

        // With the served files gone, a later build ships from the kept downloads.
        std::fs::remove_dir_all(&served).expect("remove the served files");
        let again = dir.join("native-again");
        std::fs::create_dir_all(&again).expect("make a second native folder");
        ship(&files, &[], &again, &downloads).expect("ship from the kept downloads");
        assert_eq!(std::fs::read(again.join("dxcompiler.dll")).expect("the dll again"), b"dxc");
        std::fs::remove_dir_all(&dir).expect("remove the scratch folder");
    }

    #[test]
    fn a_download_with_another_hash_or_lacking_a_file_is_refused_and_the_library_is_never_replaced() {
        let dir = scratch("refuse");
        let native = dir.join("native");
        let downloads = dir.join("downloads");
        std::fs::create_dir_all(&native).expect("make the native folder");
        std::fs::write(native.join("demo.dll"), b"the library").expect("write the library");
        let zip = zip_of(&[("a.dll", b"a"), ("LICENSE", b"terms")]);
        let served = dir.join("a.zip");
        std::fs::write(&served, &zip).expect("serve the zip");
        let entry = |name: &str, sha256: &str, path: &str, license: &str| {
            format!(r#"{{ "{name}": {{ "url": "{}", "sha256": "{sha256}", "path": "{path}", "licenses": ["{license}"] }} }}"#, file_url(&served))
        };
        let ship_one = |json: &str| ship(&from_metadata(&table(json), &dir, SOURCE).expect("a well formed entry"), &[], &native, &downloads);
        let pinned = sha256_text(&zip);
        let wrong = ship_one(&entry("a.dll", PIN, "a.dll", "LICENSE")).expect_err("a download with another hash");
        assert!(wrong.to_string().contains(&format!("has SHA-256 {pinned}, and the entry pins {PIN}")), "{wrong}");
        assert!(!downloads.join(PIN).join("a.zip").exists(), "nothing of a refused download is kept");
        let missing = ship_one(&entry("a.dll", &pinned, "b.dll", "LICENSE")).expect_err("an archive without the file");
        assert!(missing.to_string().contains("holds no b.dll"), "{missing}");
        let unlicensed = ship_one(&entry("a.dll", &pinned, "a.dll", "COPYING")).expect_err("an archive without the license file");
        assert!(unlicensed.to_string().contains("holds no license file COPYING"), "{unlicensed}");
        let replacing = ship_one(&entry("Demo.dll", &pinned, "a.dll", "LICENSE")).expect_err("an entry naming the library");
        assert!(replacing.to_string().contains("would replace demo.dll"), "{replacing}");
        assert_eq!(std::fs::read(native.join("demo.dll")).expect("the library"), b"the library", "the library is left as it was");
        std::fs::remove_dir_all(&dir).expect("remove the scratch folder");
    }
}
