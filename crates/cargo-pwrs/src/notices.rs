//! The license notices a module's native library owes, and the check
//! that every crate compiled into it is under a license the module
//! allows.
//!
//! A native library carries every crate it links, so shipping it
//! redistributes each of them in binary form. `write` puts the license
//! of each such crate, and the license texts its source carries, into
//! `THIRD-PARTY-NOTICES.txt` beside the library in `runtimes/<rid>/`,
//! so a folder merged from several platforms keeps each library's own
//! list. A crate whose license expression the allowed licenses do not
//! satisfy stops the build.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Error;

/// The notices file's name, beside each native library and at the root
/// of the module folder.
pub const FILE_NAME: &str = "THIRD-PARTY-NOTICES.txt";

/// PWRS's own license, which covers the managed runtime compiled into
/// every module.
const PWRS_LICENSE: &str = include_str!("../LICENSE");

/// The MIT license's terms after its copyright line, for a package that
/// declares MIT and ships no license file of its own.
const MIT_TERMS: &str = "Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the \"Software\"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.";

const RULE: &str = "================================================================================";
const THIN: &str = "--------------------------------------------------------------------------------";

/// The licenses a module may compile in without naming them in
/// `[package.metadata.pwrs] allowed-licenses`: permissive licenses
/// whose terms a notices file meets.
pub const DEFAULT_ALLOWED: &[&str] = &[
    "0BSD",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSL-1.0",
    "CC0-1.0",
    "CDLA-Permissive-2.0",
    "ISC",
    "MIT",
    "MIT-0",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "Unlicense",
    "Zlib",
];

/// A license expression as SPDX writes one.
#[derive(Debug, PartialEq)]
enum Expr {
    License(String),
    With(String, String),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

/// Splits an expression into names, operators and parentheses. `/`,
/// which crates published before cargo took SPDX expressions put
/// between alternatives, reads as `OR`.
fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in text.chars() {
        if c == '(' || c == ')' || c == '/' || c.is_whitespace() {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            match c {
                '/' => out.push("OR".to_string()),
                '(' | ')' => out.push(c.to_string()),
                _space => {}
            }
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

fn is_operator(token: &str) -> bool {
    ["AND", "OR", "WITH"].iter().any(|op| token.eq_ignore_ascii_case(op))
}

/// Recursive descent over `tokens`, `OR` binding loosest and `WITH`
/// tightest, as SPDX orders them.
struct Parser {
    tokens: Vec<String>,
    at: usize,
}

impl Parser {
    fn next_is(&self, op: &str) -> bool {
        self.tokens.get(self.at).is_some_and(|t| t.eq_ignore_ascii_case(op))
    }

    fn any(&mut self) -> Result<Expr, String> {
        let mut left = self.all()?;
        while self.next_is("OR") {
            self.at += 1;
            left = Expr::Or(Box::new(left), Box::new(self.all()?));
        }
        Ok(left)
    }

    fn all(&mut self) -> Result<Expr, String> {
        let mut left = self.with()?;
        while self.next_is("AND") {
            self.at += 1;
            left = Expr::And(Box::new(left), Box::new(self.with()?));
        }
        Ok(left)
    }

    fn with(&mut self) -> Result<Expr, String> {
        let base = self.primary()?;
        if !self.next_is("WITH") {
            return Ok(base);
        }
        self.at += 1;
        let exception = self.name()?;
        match base {
            Expr::License(id) => Ok(Expr::With(id, exception)),
            _grouped => Err("WITH follows a parenthesized expression rather than a license".to_string()),
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        if self.tokens.get(self.at).map(String::as_str) != Some("(") {
            return Ok(Expr::License(self.name()?));
        }
        self.at += 1;
        let inner = self.any()?;
        if self.tokens.get(self.at).map(String::as_str) != Some(")") {
            return Err("a parenthesis is not closed".to_string());
        }
        self.at += 1;
        Ok(inner)
    }

    fn name(&mut self) -> Result<String, String> {
        match self.tokens.get(self.at) {
            Some(t) if t != "(" && t != ")" && !is_operator(t) => {
                self.at += 1;
                Ok(t.clone())
            }
            Some(t) => Err(format!("a license name was expected where it says {t}")),
            None => Err("the expression ends where a license name belongs".to_string()),
        }
    }
}

fn parse(text: &str) -> Result<Expr, String> {
    let mut p = Parser { tokens: tokens(text), at: 0 };
    if p.tokens.is_empty() {
        return Err("the expression is empty".to_string());
    }
    let expr = p.any()?;
    match p.tokens.get(p.at) {
        Some(extra) => Err(format!("{extra} follows a complete expression")),
        None => Ok(expr),
    }
}

/// The license a trailing `+` (this version or any later one) names.
fn without_or_later(id: &str) -> &str {
    match id.strip_suffix('+') {
        Some(base) => base,
        None => id,
    }
}

/// A license name with runs of spaces made one, so an allowed
/// `Apache-2.0  WITH LLVM-exception` compares as SPDX writes it.
fn normalize(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether the crate can be shipped under `expr` with only the
/// licenses in `allowed`: an `OR` needs one side, an `AND` both.
/// Names compare without regard to case, as SPDX matches them.
fn satisfied(expr: &Expr, allowed: &[String]) -> bool {
    let has = |name: &str| allowed.iter().any(|a| a.eq_ignore_ascii_case(name));
    match expr {
        Expr::License(id) => has(without_or_later(id)),
        Expr::With(id, exception) => has(&format!("{} WITH {exception}", without_or_later(id))),
        Expr::And(a, b) => satisfied(a, allowed) && satisfied(b, allowed),
        Expr::Or(a, b) => satisfied(a, allowed) || satisfied(b, allowed),
    }
}

/// The defaults with the manifest's `allowed-licenses` added.
pub fn allowed(extra: &[String]) -> Vec<String> {
    DEFAULT_ALLOWED.iter().map(|s| s.to_string()).chain(extra.iter().map(|s| normalize(s))).collect()
}

/// One crate the native library links.
#[derive(Debug)]
pub struct Linked {
    pub name: String,
    pub version: String,
    /// `license`, the SPDX expression, when the crate declares one.
    pub license: Option<String>,
    /// `license-file`, as a path within the crate's folder.
    pub license_file: Option<PathBuf>,
    /// The folder holding the crate's `Cargo.toml`.
    pub folder: PathBuf,
    pub repository: Option<String>,
}

/// The crates the library of the package whose manifest is `manifest`
/// links when built for `triple` with `feature_args`: everything its
/// normal dependencies reach, the package itself left out. A proc
/// macro, and whatever only it depends on, runs in the compiler and is
/// not linked, and neither is a build dependency.
pub fn linked_crates(manifest: &Path, triple: &str, feature_args: &[String]) -> Result<Vec<Linked>, Error> {
    let out = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--filter-platform", triple, "--manifest-path"])
        .arg(manifest)
        .args(feature_args)
        .output()
        .map_err(|e| Error::msg(format!("cannot run cargo metadata: {e}")))?;
    if !out.status.success() {
        return Err(Error::msg(format!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr))));
    }
    let meta: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| Error::msg(format!("cargo metadata is not JSON: {e}")))?;
    from_metadata(&meta, manifest)
}

fn canonical(path: &Path) -> Result<PathBuf, Error> {
    std::fs::canonicalize(path).map_err(|e| Error::msg(format!("cannot resolve {}: {e}", path.display())))
}

fn from_metadata(meta: &serde_json::Value, manifest: &Path) -> Result<Vec<Linked>, Error> {
    let packages = meta["packages"].as_array().ok_or_else(|| Error::msg("cargo metadata has no packages"))?;
    let mut by_id: HashMap<&str, &serde_json::Value> = HashMap::new();
    let wanted = canonical(manifest)?;
    let mut root = None;
    for p in packages {
        let id = p["id"].as_str().ok_or_else(|| Error::msg("cargo metadata lists a package without an id"))?;
        let m = p["manifest_path"].as_str().ok_or_else(|| Error::msg(format!("{id} has no manifest path")))?;
        if canonical(Path::new(m))? == wanted {
            root = Some(id);
        }
        by_id.insert(id, p);
    }
    let root = root.ok_or_else(|| Error::msg(format!("cargo metadata lists no package with the manifest {}", manifest.display())))?;
    let nodes = meta["resolve"]["nodes"].as_array().ok_or_else(|| Error::msg("cargo metadata has no resolve graph"))?;
    let mut deps_of: HashMap<&str, &Vec<serde_json::Value>> = HashMap::new();
    for n in nodes {
        let id = n["id"].as_str().ok_or_else(|| Error::msg("the resolve graph has a node without an id"))?;
        let deps = n["deps"].as_array().ok_or_else(|| Error::msg(format!("the resolve graph's node {id} has no dependency list")))?;
        deps_of.insert(id, deps);
    }
    let is_proc_macro = |p: &serde_json::Value| -> bool {
        p["targets"].as_array().is_some_and(|ts| {
            ts.iter().any(|t| t["kind"].as_array().is_some_and(|k| k.iter().any(|k| k.as_str() == Some("proc-macro"))))
        })
    };

    let mut seen: HashSet<&str> = HashSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let deps = deps_of.get(id).ok_or_else(|| Error::msg(format!("the resolve graph has no node for {id}")))?;
        for dep in deps.iter() {
            let pkg = dep["pkg"].as_str().ok_or_else(|| Error::msg(format!("a dependency of {id} names no package")))?;
            let package = by_id.get(pkg).ok_or_else(|| Error::msg(format!("the resolve graph names {pkg}, which cargo metadata does not list")))?;
            let kinds = dep["dep_kinds"].as_array().ok_or_else(|| Error::msg(format!("the dependency of {id} on {pkg} has no kinds")))?;
            let normal = kinds.iter().any(|k| k["kind"].is_null());
            if normal && !is_proc_macro(package) && seen.insert(pkg) {
                stack.push(pkg);
            }
        }
    }

    let mut linked = Vec::with_capacity(seen.len());
    for id in seen {
        let p = by_id[id];
        let text = |key: &str| p[key].as_str().filter(|s| !s.trim().is_empty()).map(String::from);
        let manifest_path = p["manifest_path"].as_str().ok_or_else(|| Error::msg(format!("{id} has no manifest path")))?;
        let folder = Path::new(manifest_path).parent().ok_or_else(|| Error::msg(format!("{manifest_path} has no folder")))?.to_path_buf();
        linked.push(Linked {
            name: text("name").ok_or_else(|| Error::msg(format!("{id} has no name")))?,
            version: text("version").ok_or_else(|| Error::msg(format!("{id} has no version")))?,
            license: text("license"),
            license_file: text("license_file").map(PathBuf::from),
            folder,
            repository: text("repository"),
        });
    }
    linked.sort_by(|a, b| (a.name.as_str(), a.version.as_str()).cmp(&(b.name.as_str(), b.version.as_str())));
    Ok(linked)
}

/// Why a linked crate cannot ship under the module's allowed licenses,
/// or `None` when it can.
pub fn refusal(c: &Linked, allowed: &[String]) -> Option<String> {
    match (&c.license, &c.license_file) {
        (Some(expr), _file) => match parse(expr) {
            Err(why) => Some(format!("its license `{expr}` is not an SPDX expression: {why}")),
            Ok(parsed) if satisfied(&parsed, allowed) => None,
            Ok(_not_allowed) => Some(format!("its license `{expr}` is not among the allowed licenses")),
        },
        (None, Some(file)) => Some(format!("it declares no SPDX license, only the license file {}", file.display())),
        (None, None) => Some("it declares no license, so it cannot be redistributed".to_string()),
    }
}

/// Names a file in a crate's folder that carries license terms: a
/// license, copying, notice or copyright file, alone or with a suffix
/// such as `LICENSE-MIT` or `NOTICE.txt`.
fn is_license_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["license", "licence", "copying", "notice", "copyright", "unlicense"].iter().any(|stem| {
        lower == *stem || lower.strip_prefix(stem).is_some_and(|rest| rest.starts_with(['-', '.', '_']))
    })
}

/// The license files at the top of a crate's folder and its
/// `license-file`, by name, each read as text.
pub fn license_texts(c: &Linked) -> Result<BTreeMap<String, String>, Error> {
    let mut texts = BTreeMap::new();
    let entries = std::fs::read_dir(&c.folder).map_err(|e| Error::msg(format!("cannot read {}: {e}", c.folder.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::msg(format!("cannot read an entry of {}: {e}", c.folder.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if is_license_file(&name) && path.is_file() {
            let bytes = std::fs::read(&path).map_err(|e| Error::msg(format!("cannot read {}: {e}", path.display())))?;
            texts.insert(name, String::from_utf8_lossy(&bytes).into_owned());
        }
    }
    if let Some(file) = &c.license_file {
        let path = c.folder.join(file);
        let bytes = std::fs::read(&path).map_err(|e| Error::msg(format!("{} {}'s license-file {}: {e}", c.name, c.version, path.display())))?;
        texts.insert(file.display().to_string(), String::from_utf8_lossy(&bytes).into_owned());
    }
    Ok(texts)
}

/// A license file a module supplies for a crate whose package ships
/// none, from `[package.metadata.pwrs.license-files]` or from the
/// `license-files` of a bundling module's `bundled-modules` entry.
#[derive(Debug, Clone)]
pub struct Supplied {
    pub name: String,
    pub version: String,
    /// The path as the manifest writes it, which the notices name.
    pub written: String,
    /// The path resolved against the folder of the package that wrote it.
    pub path: PathBuf,
    /// Where the entry is written, as refusals name it.
    pub source: String,
    /// The module supplying the file when it is not the library's own:
    /// the module bundling it.
    pub supplier: Option<String>,
}

/// A `license-files` table: each key `name@version`, each value a path
/// relative to `package_dir`. `source` names the table in refusals, and
/// `supplier` is the bundling module that supplies the files for a
/// bundled library. An absent table is no entries.
pub fn supplied_from_metadata(table: &serde_json::Value, package_dir: &Path, source: &str, supplier: Option<&str>) -> Result<Vec<Supplied>, Error> {
    if table.is_null() {
        return Ok(Vec::new());
    }
    let map = table.as_object().ok_or_else(|| Error::msg(format!("{source} is a table of \"name@version\" = \"path\"")))?;
    let mut out = Vec::with_capacity(map.len());
    for (key, value) in map {
        let (name, version) = match key.rsplit_once('@') {
            Some((n, v)) if !n.is_empty() && !v.is_empty() => (n, v),
            _not_pinned => return Err(Error::msg(format!("{source} key `{key}` is not `name@version`"))),
        };
        let written = value.as_str().ok_or_else(|| Error::msg(format!("{source} `{key}` is not a path")))?;
        out.push(Supplied {
            name: name.to_string(),
            version: version.to_string(),
            written: written.to_string(),
            path: package_dir.join(written),
            source: source.to_string(),
            supplier: supplier.map(String::from),
        });
    }
    Ok(out)
}

/// A supplied license file checked against the library: the path as the
/// manifest writes it, the file's text, and the bundling module that
/// supplies it for a bundled library.
#[derive(Debug)]
pub struct SuppliedText {
    pub written: String,
    pub text: String,
    pub supplier: Option<String>,
}

/// Supplied license files by `(name, version)`.
pub type SuppliedTexts = BTreeMap<(String, String), SuppliedText>;

/// Each supplied license file's text, keyed by `(name, version)`, after
/// checking it against the crates the library links: it names a linked
/// crate at a linked version, that crate's package ships no license file
/// of its own, no other entry supplies the same crate and version, and
/// the file is there and not empty.
pub fn supplied_texts(supplied: &[Supplied], linked: &[Linked]) -> Result<SuppliedTexts, Error> {
    let mut out = BTreeMap::new();
    let mut sources: BTreeMap<(String, String), &str> = BTreeMap::new();
    for s in supplied {
        let source = s.source.as_str();
        let Some(c) = linked.iter().find(|c| c.name == s.name && c.version == s.version) else {
            let versions: Vec<&str> = linked.iter().filter(|c| c.name == s.name).map(|c| c.version.as_str()).collect();
            let linked_as = if versions.is_empty() { "does not link it".to_string() } else { format!("links it at {}", versions.join(", ")) };
            return Err(Error::msg(format!("{source} names {}@{}, and the library {linked_as}", s.name, s.version)));
        };
        let own = license_texts(c)?;
        if !own.is_empty() {
            let names: Vec<&str> = own.keys().map(String::as_str).collect();
            return Err(Error::msg(format!(
                "{} {} ships its own license file ({}); remove its {source} entry",
                s.name,
                s.version,
                names.join(", ")
            )));
        }
        let key = (s.name.clone(), s.version.clone());
        if let Some(first) = sources.get(&key) {
            return Err(Error::msg(format!("{}@{} is supplied twice, by {first} and by {source}; keep one of them", s.name, s.version)));
        }
        let bytes = std::fs::read(&s.path).map_err(|e| Error::msg(format!("{source} {}@{}: cannot read {}: {e}", s.name, s.version, s.path.display())))?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if text.trim().is_empty() {
            return Err(Error::msg(format!("{source} {}@{}: {} is empty", s.name, s.version, s.path.display())));
        }
        sources.insert(key.clone(), source);
        out.insert(key, SuppliedText { written: s.written.clone(), text, supplier: s.supplier.clone() });
    }
    Ok(out)
}

/// The notices for one native library: each crate it links, with the
/// license it declares and the license files its source carries, or the
/// one the module, or the module bundling it, supplies for it. Returns
/// the text, and the crates, as `name version`, left without any license
/// text.
pub fn native_notices(linked: &[Linked], supplied: &SuppliedTexts, module: &str, library: &str) -> Result<(String, Vec<String>), Error> {
    let mut out = format!(
        "Third-party notices for {library}, a native library of the {module} module\n\n\
         The library compiles in the crates below. Each is listed with the license it\n\
         declares and the license files its source carries.\n"
    );
    let mut without_text = Vec::new();
    for c in linked {
        let license = c.license.as_deref().ok_or_else(|| Error::msg(format!("{} {} declares no license expression to list", c.name, c.version)))?;
        out.push_str(&format!("\n{RULE}\n{} {}\nLicense: {license}\n", c.name, c.version));
        if let Some(repository) = &c.repository {
            out.push_str(&format!("Repository: {repository}\n"));
        }
        let texts = license_texts(c)?;
        if texts.is_empty() {
            match supplied.get(&(c.name.clone(), c.version.clone())) {
                Some(given) => {
                    let by = given.supplier.as_deref().unwrap_or(module);
                    out.push_str(&format!("No license file ships in the crate's package; the {by} module supplies it.\n"));
                    out.push_str(&format!("{THIN}\n{}\n{THIN}\n{}\n", given.written, given.text.trim_end()));
                }
                None => {
                    out.push_str("The crate's source carries no license file.\n");
                    without_text.push(format!("{} {}", c.name, c.version));
                }
            }
        }
        for (name, text) in &texts {
            out.push_str(&format!("{THIN}\n{name}\n{THIN}\n{}\n", text.trim_end()));
        }
    }
    Ok((out, without_text))
}

/// The text of the first element named `tag` in a nuspec, whether or
/// not it carries attributes.
fn element<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut from = 0;
    while let Some(found) = xml[from..].find(&open) {
        let after = from + found + open.len();
        if xml[after..].starts_with(['>', ' ', '\t', '\r', '\n']) {
            let start = after + xml[after..].find('>')? + 1;
            let end = start + xml[start..].find(&close)?;
            return Some(xml[start..end].trim());
        }
        from = after;
    }
    None
}

/// What a NuGet package's `.nuspec` says of the package an assembly
/// came from, found in the package folder above `lib/<framework>/`.
struct Package {
    id: String,
    version: String,
    license: String,
    copyright: String,
    project: String,
}

fn package_of(assembly: &Path) -> Result<Package, Error> {
    let folder = assembly
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| Error::msg(format!("{} is not in a package's lib/<framework> folder", assembly.display())))?;
    let entries = std::fs::read_dir(folder).map_err(|e| Error::msg(format!("cannot read {}: {e}", folder.display())))?;
    let mut nuspec = None;
    for entry in entries {
        let path = entry.map_err(|e| Error::msg(format!("cannot read an entry of {}: {e}", folder.display())))?.path();
        if path.extension().is_some_and(|x| x == "nuspec") {
            nuspec = Some(path);
        }
    }
    let nuspec = nuspec.ok_or_else(|| Error::msg(format!("{} holds no .nuspec", folder.display())))?;
    let xml = std::fs::read_to_string(&nuspec).map_err(|e| Error::msg(format!("cannot read {}: {e}", nuspec.display())))?;
    let field = |tag: &str| -> Result<String, Error> {
        element(&xml, tag).map(String::from).ok_or_else(|| Error::msg(format!("{} has no <{tag}>", nuspec.display())))
    };
    Ok(Package { id: field("id")?, version: field("version")?, license: field("license")?, copyright: field("copyright")?, project: field("projectUrl")? })
}

/// The notices for what cargo-pwrs itself puts in every module folder:
/// the managed runtime compiled from PWRS's C#, and `desktop`, the
/// .NET Framework assemblies a module with hand-written C# ships in
/// netstandard2.0, each from a NuGet package that declares MIT.
pub fn root_notices(module: &str, desktop: &[PathBuf]) -> Result<String, Error> {
    let mut out = format!(
        "Third-party notices for the {module} module\n\n\
         {RULE}\nPWRS\n{}\n\
         Pwrs.Bootstrap.dll and Pwrs.Runtime.dll in net10.0 and netstandard2.0 are\n\
         compiled from PWRS's C# sources, and each {module}.Shell assembly from C#\n\
         that PWRS generates for the module.\nLicense: MIT\n{THIN}\n{}\n",
        env!("CARGO_PKG_REPOSITORY"),
        PWRS_LICENSE.trim_end()
    );
    for assembly in desktop {
        let p = package_of(assembly)?;
        if !p.license.eq_ignore_ascii_case("MIT") {
            return Err(Error::msg(format!(
                "{} {} declares the license {}, and the notices quote MIT's terms for the assemblies a module ships from NuGet",
                p.id, p.version, p.license
            )));
        }
        let file = match assembly.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => return Err(Error::msg(format!("{} has no file name", assembly.display()))),
        };
        out.push_str(&format!(
            "\n{RULE}\n{} {}\n{}\nnetstandard2.0/{file}, from the NuGet package of that name.\nLicense: MIT\n{THIN}\n{}\n\n{MIT_TERMS}\n",
            p.id, p.version, p.project, p.copyright
        ));
    }
    out.push_str(&format!(
        "\n{RULE}\nThe native libraries\n\
         Each runtimes/<rid>/{FILE_NAME} lists the crates compiled into the native\n\
         library in that folder, with their licenses.\n"
    ));
    Ok(out)
}

/// Writes `text` to `FILE_NAME` in `folder`.
pub fn write(folder: &Path, text: &str) -> Result<(), Error> {
    std::fs::create_dir_all(folder).map_err(|e| Error::msg(format!("cannot create {}: {e}", folder.display())))?;
    let path = folder.join(FILE_NAME);
    std::fs::write(&path, text).map_err(|e| Error::msg(format!("cannot write {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(expr: &str, extra: &[&str]) -> bool {
        let extra: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
        let parsed = parse(expr).unwrap_or_else(|e| panic!("{expr} did not parse: {e}"));
        satisfied(&parsed, &allowed(&extra))
    }

    #[test]
    fn expressions_are_judged_against_the_allowed_licenses() {
        assert!(ok("MIT", &[]));
        assert!(ok("mit", &[]), "names compare without regard to case");
        assert!(ok("MIT OR Apache-2.0", &[]));
        assert!(ok("MIT/Apache-2.0", &[]), "a slash reads as OR");
        assert!(ok("(MIT OR Apache-2.0) AND Unicode-3.0", &[]));
        assert!(ok("Apache-2.0 WITH LLVM-exception", &[]));
        assert!(ok(
            "ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0)",
            &[]
        ));
        assert!(ok("GPL-3.0 OR MIT", &[]), "one allowed side of an OR is enough");
        assert!(!ok("MIT AND GPL-3.0", &[]), "every side of an AND must be allowed");
        assert!(!ok("GPL-2.0 WITH Classpath-exception-2.0", &[]));
        assert!(!ok("LGPL-2.0-or-later", &[]));
        assert!(!ok("MPL-2.0", &[]));
        assert!(ok("MPL-2.0", &["MPL-2.0"]), "the manifest's list adds to the defaults");
        assert!(ok("MPL-2.0+", &["MPL-2.0"]), "a trailing + is met by the version it names");
        assert!(ok("GPL-2.0 WITH Classpath-exception-2.0", &["GPL-2.0  WITH  Classpath-exception-2.0"]));
    }

    #[test]
    fn a_malformed_expression_is_an_error() {
        for bad in ["", "MIT AND", "(MIT", "MIT)", "AND MIT", "(MIT OR ISC) WITH LLVM-exception", "MIT ISC"] {
            assert!(parse(bad).is_err(), "{bad:?} parsed");
        }
    }

    fn crate_with(license: Option<&str>, license_file: Option<&str>) -> Linked {
        Linked {
            name: "dep".to_string(),
            version: "1.0.0".to_string(),
            license: license.map(String::from),
            license_file: license_file.map(PathBuf::from),
            folder: PathBuf::from("."),
            repository: None,
        }
    }

    #[test]
    fn each_refusal_names_its_reason() {
        let allowed = allowed(&[]);
        assert_eq!(refusal(&crate_with(Some("MIT OR Apache-2.0"), None), &allowed), None);
        let why = |c: Linked| refusal(&c, &allowed).expect("refused");
        assert!(why(crate_with(Some("LGPL-2.0-or-later"), None)).contains("not among the allowed licenses"));
        assert!(why(crate_with(Some("MIT AND"), None)).contains("not an SPDX expression"));
        assert!(why(crate_with(None, Some("LICENSE"))).contains("only the license file LICENSE"));
        assert!(why(crate_with(None, None)).contains("declares no license"));
    }

    #[test]
    fn license_files_are_recognized_by_name() {
        for yes in ["LICENSE", "LICENSE-MIT", "license.txt", "LICENCE", "COPYING", "NOTICE", "notice.md", "COPYRIGHT", "UNLICENSE"] {
            assert!(is_license_file(yes), "{yes}");
        }
        for no in ["README.md", "licensing.md", "LICENSES", "Cargo.toml", "noticeable.rs"] {
            assert!(!is_license_file(no), "{no}");
        }
    }

    /// A resolve graph in cargo metadata's shape over folders that exist,
    /// since packages are matched by their resolved manifest paths.
    fn graph(dir: &Path) -> serde_json::Value {
        let pkg = |name: &str, kind: &str| {
            let folder = dir.join(name);
            std::fs::create_dir_all(&folder).expect("create a package folder");
            let manifest = folder.join("Cargo.toml");
            std::fs::write(&manifest, "").expect("write a manifest");
            serde_json::json!({
                "id": name, "name": name, "version": "1.0.0", "license": "MIT",
                "manifest_path": manifest.display().to_string(),
                "targets": [{ "kind": [kind] }]
            })
        };
        let dep = |pkg: &str, kind: serde_json::Value| serde_json::json!({ "pkg": pkg, "dep_kinds": [{ "kind": kind }] });
        serde_json::json!({
            "packages": [pkg("module", "cdylib"), pkg("a", "lib"), pkg("b", "lib"), pkg("macro", "proc-macro"),
                         pkg("syn", "lib"), pkg("cc", "lib"), pkg("tester", "lib")],
            "resolve": { "nodes": [
                { "id": "module", "deps": [dep("a", serde_json::Value::Null), dep("macro", serde_json::Value::Null),
                                           dep("cc", "build".into()), dep("tester", "dev".into())] },
                { "id": "a", "deps": [dep("b", serde_json::Value::Null)] },
                { "id": "b", "deps": [] },
                { "id": "macro", "deps": [dep("syn", serde_json::Value::Null)] },
                { "id": "syn", "deps": [] },
                { "id": "cc", "deps": [] },
                { "id": "tester", "deps": [] }
            ] }
        })
    }

    #[test]
    fn only_crates_the_library_links_are_listed() {
        let dir = std::env::temp_dir().join(format!("pwrs-notices-graph-{}", std::process::id()));
        let meta = graph(&dir);
        let linked = from_metadata(&meta, &dir.join("module").join("Cargo.toml")).expect("walk the graph");
        let names: Vec<&str> = linked.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["a", "b"], "a proc macro and what only it needs, a build dependency and a dev dependency are not linked");
    }

    #[test]
    fn license_texts_come_from_the_crate_folder_and_its_license_file() {
        let dir = std::env::temp_dir().join(format!("pwrs-notices-texts-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("docs")).expect("create the crate folder");
        std::fs::write(dir.join("LICENSE-MIT"), "mit terms").expect("write LICENSE-MIT");
        std::fs::write(dir.join("README.md"), "not a license").expect("write README.md");
        std::fs::write(dir.join("docs").join("TERMS"), "own terms").expect("write the license file");
        let mut c = crate_with(None, Some("docs/TERMS"));
        c.folder = dir.clone();
        let texts = license_texts(&c).expect("read the texts");
        let names: Vec<&str> = texts.keys().map(String::as_str).collect();
        assert_eq!(names, ["LICENSE-MIT", "docs/TERMS"]);
        assert_eq!(texts["docs/TERMS"], "own terms");
    }

    #[test]
    fn native_notices_quote_each_crates_files_and_name_the_ones_without() {
        let dir = std::env::temp_dir().join(format!("pwrs-notices-native-{}", std::process::id()));
        let with = dir.join("with");
        let without = dir.join("without");
        std::fs::create_dir_all(&with).expect("create a crate folder");
        std::fs::create_dir_all(&without).expect("create a crate folder");
        std::fs::write(with.join("LICENSE-MIT"), "Copyright (c) someone\n\nmit terms\n").expect("write LICENSE-MIT");
        let mut quoted = crate_with(Some("MIT OR Apache-2.0"), None);
        quoted.name = "quoted".to_string();
        quoted.folder = with;
        quoted.repository = Some("https://example.invalid/quoted".to_string());
        let mut bare = crate_with(Some("MIT"), None);
        bare.name = "bare".to_string();
        bare.folder = without;
        let (text, missing) = native_notices(&[quoted, bare], &BTreeMap::new(), "Demo", "demo.dll").expect("write the notices");
        assert!(text.starts_with("Third-party notices for demo.dll, a native library of the Demo module"), "{text}");
        assert!(text.contains("quoted 1.0.0\nLicense: MIT OR Apache-2.0\nRepository: https://example.invalid/quoted\n"), "{text}");
        assert!(text.contains("LICENSE-MIT\n-----"), "{text}");
        assert!(text.contains("Copyright (c) someone\n\nmit terms\n"), "{text}");
        assert!(text.contains("bare 1.0.0\nLicense: MIT\nThe crate's source carries no license file.\n"), "{text}");
        assert_eq!(missing, ["bare 1.0.0"]);
    }

    #[test]
    fn a_supplied_license_file_fills_only_a_linked_crate_that_ships_none() {
        let dir = std::env::temp_dir().join(format!("pwrs-notices-supplied-{}", std::process::id()));
        let with = dir.join("with");
        let without = dir.join("without");
        let module = dir.join("module");
        for d in [&with, &without, &module] {
            std::fs::create_dir_all(d).expect("create a folder");
        }
        std::fs::write(with.join("LICENSE"), "its own terms").expect("write LICENSE");
        std::fs::write(module.join("bare-LICENSE.txt"), "BSD 3-Clause terms\n").expect("write the supplied file");
        std::fs::write(module.join("empty.txt"), " \n").expect("write an empty file");
        let mut quoted = crate_with(Some("MIT"), None);
        quoted.name = "quoted".to_string();
        quoted.folder = with;
        let mut bare = crate_with(Some("BSD-3-Clause"), None);
        bare.name = "bare".to_string();
        bare.folder = without;
        let linked = [quoted, bare];
        let table = |json: &str| -> serde_json::Value { serde_json::from_str(json).expect("json") };
        let own = "[package.metadata.pwrs] license-files";
        let supply = |json: &str| supplied_from_metadata(&table(json), &module, own, None).and_then(|s| supplied_texts(&s, &linked));

        let texts = supply(r#"{"bare@1.0.0": "bare-LICENSE.txt"}"#).expect("a crate that ships none takes one");
        let (text, missing) = native_notices(&linked, &texts, "Demo", "demo.dll").expect("write the notices");
        assert!(text.contains("bare 1.0.0\nLicense: BSD-3-Clause\nNo license file ships in the crate's package; the Demo module supplies it.\n"), "{text}");
        assert!(text.contains("bare-LICENSE.txt\n-----"), "{text}");
        assert!(text.contains("BSD 3-Clause terms\n"), "{text}");
        assert!(missing.is_empty(), "{missing:?}");

        let refused = |json: &str, why: &str| match supply(json) {
            Ok(texts) => panic!("{json} was accepted: {texts:?}"),
            Err(e) => assert!(e.to_string().contains(why), "{json}: {e}"),
        };
        refused(r#"{"elsewhere@1.0.0": "bare-LICENSE.txt"}"#, "names elsewhere@1.0.0, and the library does not link it");
        refused(r#"{"bare@2.0.0": "bare-LICENSE.txt"}"#, "names bare@2.0.0, and the library links it at 1.0.0");
        refused(r#"{"quoted@1.0.0": "bare-LICENSE.txt"}"#, "quoted 1.0.0 ships its own license file (LICENSE)");
        refused(r#"{"bare@1.0.0": "empty.txt"}"#, "is empty");
        refused(r#"{"bare@1.0.0": "absent.txt"}"#, "cannot read");
        refused(r#"{"bare": "bare-LICENSE.txt"}"#, "key `bare` is not `name@version`");
        refused(r#"{"bare@1.0.0": 7}"#, "`bare@1.0.0` is not a path");
        assert!(supplied_from_metadata(&serde_json::Value::Null, &module, own, None).expect("no table").is_empty());

        // A bundling module's entry supplies the file for the bundled
        // library, and the notices name the bundling module; the same
        // crate and version supplied from two places is refused.
        let entry = "the bundled-modules entry ../demo of outer, license-files";
        let theirs = supplied_from_metadata(&table(r#"{"bare@1.0.0": "bare-LICENSE.txt"}"#), &module, entry, Some("Outer")).expect("an entry's table");
        let texts = supplied_texts(&theirs, &linked).expect("the entry's file is taken");
        let (text, missing) = native_notices(&linked, &texts, "Demo", "demo.dll").expect("write the notices");
        assert!(text.contains("No license file ships in the crate's package; the Outer module supplies it.\n"), "{text}");
        assert!(missing.is_empty(), "{missing:?}");
        let mut both = supplied_from_metadata(&table(r#"{"bare@1.0.0": "bare-LICENSE.txt"}"#), &module, own, None).expect("the own table");
        both.extend(theirs);
        match supplied_texts(&both, &linked) {
            Ok(texts) => panic!("a crate supplied twice was accepted: {texts:?}"),
            Err(e) => assert!(e.to_string().contains(&format!("bare@1.0.0 is supplied twice, by {own} and by {entry}")), "{e}"),
        }
        std::fs::remove_dir_all(&dir).expect("remove the scratch folder");
    }

    #[test]
    fn nuspec_elements_are_read_by_their_exact_name() {
        let xml = r#"<metadata><licenseUrl>https://licenses.nuget.org/MIT</licenseUrl><license type="expression">MIT</license><id> Some.Package </id></metadata>"#;
        assert_eq!(element(xml, "license"), Some("MIT"));
        assert_eq!(element(xml, "licenseUrl"), Some("https://licenses.nuget.org/MIT"));
        assert_eq!(element(xml, "id"), Some("Some.Package"));
        assert_eq!(element(xml, "copyright"), None);
    }

    /// A NuGet package folder in the toolchain's layout holding one
    /// assembly, whose nuspec declares `license`.
    fn package(dir: &Path, id: &str, license: &str) -> PathBuf {
        let lib = dir.join(id).join("lib").join("net462");
        std::fs::create_dir_all(&lib).expect("create the package folder");
        let nuspec = format!(
            "<package><metadata><id>{id}</id><version>1.2.3</version><license type=\"expression\">{license}</license>\
             <copyright>\u{a9} Someone. All rights reserved.</copyright><projectUrl>https://example.invalid/{id}</projectUrl></metadata></package>"
        );
        std::fs::write(dir.join(id).join(format!("{id}.nuspec")), nuspec).expect("write the nuspec");
        let dll = lib.join(format!("{id}.dll"));
        std::fs::write(&dll, b"assembly").expect("write the assembly");
        dll
    }

    #[test]
    fn root_notices_cover_the_runtime_and_each_shipped_package() {
        let bare = root_notices("Demo", &[]).expect("notices without packages");
        assert!(bare.contains("Pwrs.Bootstrap.dll and Pwrs.Runtime.dll"), "{bare}");
        assert!(bare.contains(PWRS_LICENSE.trim_end()), "{bare}");
        assert!(bare.contains("Each runtimes/<rid>/THIRD-PARTY-NOTICES.txt"), "{bare}");
        assert!(!bare.contains("NuGet"), "no package ships without hand-written C#: {bare}");

        let dir = std::env::temp_dir().join(format!("pwrs-notices-root-{}", std::process::id()));
        let dll = package(&dir, "Some.Package", "MIT");
        let with = root_notices("Demo", &[dll]).expect("notices with a package");
        assert!(with.contains("Some.Package 1.2.3\nhttps://example.invalid/Some.Package\nnetstandard2.0/Some.Package.dll"), "{with}");
        assert!(with.contains("\u{a9} Someone. All rights reserved.\n\nPermission is hereby granted"), "{with}");

        let other = package(&dir, "Other.Package", "Apache-2.0");
        let refused = match root_notices("Demo", &[other]) {
            Ok(text) => panic!("a package declaring Apache-2.0 was given MIT's terms: {text}"),
            Err(e) => e.to_string(),
        };
        assert!(refused.contains("Other.Package 1.2.3 declares the license Apache-2.0"), "{refused}");
    }
}
