---
title: How To Sign A Module
weight: 9
---

Signing a module's scripts and executables with an Authenticode certificate, on Windows, Linux, FreeBSD or macOS. Source: `crates/cargo-pwrs/src/signing.rs`, `crates/cargo-pwrs/scripts/sign.ps1` and the C# in `crates/cargo-pwrs/dotnet/Pwrs.Signing/`.

## What a signature does

Windows checks the signature on a script before it runs it when the execution policy is `AllSigned`, or `RemoteSigned` for a script from the internet. A module whose `.psd1`, `.psm1` and `.ps1xml` files are signed by a publisher the machine trusts then imports. One that is not signed does not. `Install-PSResource -AuthenticodeCheck` requires a valid signature on every script and catalog in a package, and `Install-Module` checks a module's catalog when it has one. PowerShell on Linux, FreeBSD and macOS checks no signatures.

A signature names who signed and proves the file has not changed since. A timestamp from a timestamp server proves when it was signed, so the signature stays valid after the certificate expires.

## What gets signed

`cargo pwrs sign` signs every file of the module folder that can have an Authenticode signature, the bundled modules' files among them:

- each PowerShell script, manifest and XML file: `.ps1`, `.psm1`, `.psd1`, `.ps1xml`, `.psc1`, `.cdxml` and `.mof`, with the signature block at the file's end;
- each Windows executable: every `.dll` and `.exe` that is a Windows image, with the signature in its certificate table.

A file whose signature is still valid is left as it is. That is how the assemblies Microsoft signed, in `netstandard2.0`, stay signed by Microsoft. A file whose signature no longer matches its contents, because it changed after signing, is signed again, and the run names it. Other files, such as the native libraries for Linux and macOS, the notices and the help, take no signature of their own. A signed catalog covers them.

The scripts `cargo pwrs build` writes are UTF-8 with a byte order mark. Windows reads a script with no byte order mark, and no UTF-8 in its first 32 bytes, in the code page of whichever machine reads it. A signature over such a script with text beyond ASCII therefore does not verify everywhere. `cargo pwrs sign` refuses it, naming the file, before it signs anything.

## The settings

The settings go in the crate's `Cargo.toml`. Each names one key, in one of five ways:

```toml
[package.metadata.pwrs.signing]
pfx = "signing/signer.pfx"            # a PKCS#12 file with the certificate and its key
password-env = "SIGNER_PFX_PASSWORD"  # the variable set to its password
timestamp-url = "http://timestamp.digicert.com"
catalog = true
```

```toml
[package.metadata.pwrs.signing]
pem = ["signing/signer.pem", "signing/signer.key"]  # the certificates, then the private key
```

```toml
[package.metadata.pwrs.signing]
store = "0123456789ABCDEF0123456789ABCDEF01234567"  # a thumbprint in the Windows store or the macOS keychain
```

```toml
[package.metadata.pwrs.signing]
command = ["pwsh", "-NoProfile", "-File", "signing/sign-digest.ps1", "-Key", "signing/signer.key",
           "-Hash", "{hash}", "-Digest", "{digest}", "-Signature", "{signature}"]
certificate = "signing/signer.pem"    # the signer's certificate and the chain to include
```

```toml
[package.metadata.pwrs.signing]
pkcs11 = { library = "/usr/lib/softhsm/libsofthsm2.so", token = "signing", key = "code" }
certificate = "signing/signer.pem"
password-env = "TOKEN_PIN"
```

The other settings:

| Setting | What it does |
|---|---|
| `password-env` | The name of the variable set to the PFX password, the password of an encrypted PEM key, or a token's PIN. A secret is never written in the settings. |
| `certificate` | For `command` and `pkcs11`: a PEM, DER or PKCS#7 file with the signer's certificate and the certificates to include with it. |
| `timestamp-url` | An RFC 3161 timestamp server. Without one, each signature stops being valid when the certificate expires, and the run says so. |
| `hash` | `sha256`, the default, `sha384` or `sha512`. |
| `catalog` | `true` makes and signs `<Name>.cat` over every file of the folder. |
| `signer` | `pwrs`, the default, or `openauthenticode`. |

Paths are taken from the crate's folder, and a `command` runs in that folder. Each setting has a flag of the same name, which takes the value as `Cargo.toml` writes it, and a flag replaces its setting. A key flag replaces the table's key:

```text
cargo pwrs sign --pfx signer.pfx --password-env SIGNER_PFX_PASSWORD --catalog
cargo pwrs sign --pem '["signer.pem", "signer.key"]'
```

A flag's paths are taken from the folder the command runs in, and a `--command` runs there.

## Signing

```text
cargo pwrs sign                                   # build, then sign target/pwrs/<Name>
cargo pwrs sign --folder target/pwrs/<Name>       # sign a built or merged folder as it is
cargo pwrs publish --release                      # build, sign when the crate has a signing table, then publish
```

`publish --folder` packages the folder as it is and refuses a signing flag. So a merged folder is signed with `cargo pwrs sign --folder` before it is published. `merge` compares manifests without their signature blocks, so a signed folder still takes another platform's runtimes. Signing it again then signs only what the merge added, and makes the catalog again.

The run prints one line for each file, on standard error. The hello example built on Windows x64, signed with a PFX, DigiCert's timestamp server and a catalog, begins:

```text
pwrs: signing as CN=PWRS Signing Gate RSA, SHA256, timestamped by http://timestamp.digicert.com
pwrs: left Calc/netstandard2.0/Microsoft.Win32.Registry.dll as it is: it is signed already, by CN=Microsoft Corporation, OU=MOPR, O=Microsoft Corporation, L=Redmond, S=Washington, C=US
```

and ends:

```text
pwrs: signed runtimes/win-x64/native/hello-helper.exe
pwrs: signed runtimes/win-x64/native/pwrs_example_hello.dll
pwrs: made the catalog Hello.cat over the folder's 57 files
```

<details>
<summary>The whole run</summary>

```text
pwrs: signing as CN=PWRS Signing Gate RSA, SHA256, timestamped by http://timestamp.digicert.com
pwrs: left Calc/netstandard2.0/Microsoft.Win32.Registry.dll as it is: it is signed already, by CN=Microsoft Corporation, OU=MOPR, O=Microsoft Corporation, L=Redmond, S=Washington, C=US
pwrs: left Calc/netstandard2.0/System.IO.FileSystem.AccessControl.dll as it is: it is signed already, by CN=Microsoft Corporation, OU=MOPR, O=Microsoft Corporation, L=Redmond, S=Washington, C=US
pwrs: left Calc/netstandard2.0/System.Security.AccessControl.dll as it is: it is signed already, by CN=Microsoft Corporation, OU=MOPR, O=Microsoft Corporation, L=Redmond, S=Washington, C=US
pwrs: left Calc/netstandard2.0/System.Security.Principal.Windows.dll as it is: it is signed already, by CN=Microsoft Corporation, OU=MOPR, O=Microsoft Corporation, L=Redmond, S=Washington, C=US
pwrs: left netstandard2.0/Microsoft.Win32.Registry.dll as it is: it is signed already, by CN=Microsoft Corporation, OU=MOPR, O=Microsoft Corporation, L=Redmond, S=Washington, C=US
pwrs: left netstandard2.0/System.Buffers.dll as it is: it is signed already, by CN=.NET, O=Microsoft Corporation, L=Redmond, S=Washington, C=US
pwrs: left netstandard2.0/System.IO.FileSystem.AccessControl.dll as it is: it is signed already, by CN=Microsoft Corporation, OU=MOPR, O=Microsoft Corporation, L=Redmond, S=Washington, C=US
pwrs: left netstandard2.0/System.Memory.dll as it is: it is signed already, by CN=.NET, O=Microsoft Corporation, L=Redmond, S=Washington, C=US
pwrs: left netstandard2.0/System.Numerics.Vectors.dll as it is: it is signed already, by CN=.NET, O=Microsoft Corporation, L=Redmond, S=Washington, C=US
pwrs: left netstandard2.0/System.Runtime.CompilerServices.Unsafe.dll as it is: it is signed already, by CN=.NET, O=Microsoft Corporation, L=Redmond, S=Washington, C=US
pwrs: left netstandard2.0/System.Security.AccessControl.dll as it is: it is signed already, by CN=Microsoft Corporation, OU=MOPR, O=Microsoft Corporation, L=Redmond, S=Washington, C=US
pwrs: left netstandard2.0/System.Security.Principal.Windows.dll as it is: it is signed already, by CN=Microsoft Corporation, OU=MOPR, O=Microsoft Corporation, L=Redmond, S=Washington, C=US
pwrs: 25 other files take no signature of their own; the catalog covers them
pwrs: signed Calc/Calc.psd1
pwrs: signed Calc/Calc.psm1
pwrs: signed Calc/net10.0/Calc.Shell.07cc73382b6509a0.dll
pwrs: signed Calc/net10.0/Pwrs.Bootstrap.dll
pwrs: signed Calc/net10.0/Pwrs.Runtime.dll
pwrs: signed Calc/netstandard2.0/Calc.Shell.07cc73382b6509a0.dll
pwrs: signed Calc/netstandard2.0/Pwrs.Bootstrap.dll
pwrs: signed Calc/netstandard2.0/Pwrs.Runtime.dll
pwrs: signed Calc/runtimes/win-x64/native/pwrs_example_calc.dll
pwrs: signed Hello.Format.ps1xml
pwrs: signed Hello.psd1
pwrs: signed Hello.psm1
pwrs: signed net10.0/Hello.Shell.007fe2eb7a6bcf0f.dll
pwrs: signed net10.0/Pwrs.Bootstrap.dll
pwrs: signed net10.0/Pwrs.Runtime.dll
pwrs: signed netstandard2.0/Hello.Shell.007fe2eb7a6bcf0f.dll
pwrs: signed netstandard2.0/Pwrs.Bootstrap.dll
pwrs: signed netstandard2.0/Pwrs.Runtime.dll
pwrs: signed runtimes/win-x64/native/hello-helper.exe
pwrs: signed runtimes/win-x64/native/pwrs_example_hello.dll
pwrs: made the catalog Hello.cat over the folder's 57 files
```

</details>

Signed again after a line was added to `Hello.psm1`, the run ends:

```text
pwrs: left runtimes/win-x64/native/pwrs_example_hello.dll as it is: it is signed already, by CN=PWRS Signing Gate RSA
pwrs: 25 other files take no signature of their own; the catalog covers them
pwrs: signed Hello.psm1 again: its signature does not match its contents
pwrs: made the catalog Hello.cat again: Hello.psm1 has changed since it was made
```

## The key sources

A PFX file has the certificate with its key, and often the chain. When it has several certificates with keys, the one for code signing is used.

A PEM pair is a file of certificates, the signer's among them, and a file with the private key: PKCS#8, PKCS#1 or SEC1, encrypted when `password-env` is set.

`store` finds the certificate by its thumbprint in `CurrentUser\My`, then `LocalMachine\My`, on Windows, and in the default keychain on macOS. A key on a smart card that needs a PIN shows the system's prompt for it. Linux and FreeBSD have no such store, and `store` is refused there.

`command` runs a program once for each signature, with the digest to sign. Each argument may contain these placeholders:

| Placeholder | Becomes |
|---|---|
| `{digest}` | the path of a file with the digest's bytes |
| `{digest-base64}` | the digest as base64 |
| `{digest-hex}` | the digest as lowercase hexadecimal |
| `{hash}` | `sha256`, `sha384` or `sha512` |
| `{signature}` | the path of a file the program writes the signature to |

Without `{signature}`, the program prints the signature as base64. The signature is PKCS#1 v1.5 for an RSA key and DER for an ECDSA key, as key services give them. A program that exits with a code other than 0 stops the run with what it printed on standard error. Each signature is checked against the certificate's public key before it is used, so a key that does not match the certificate is named at once.

The program runs in the folder its setting came from. A program named by a relative path, such as `signing/sign.sh`, is taken from that folder too, and a bare name, such as `pwsh`, is found on `PATH`.

A command can be a key service's own tool or a script of your own. This PowerShell script is `signing/sign-digest.ps1` in the `command` example above. It signs with an RSA or ECDSA key from a PEM file, and writes the signature to the file `-Signature` names or prints it as base64:

```powershell
param([string] $Key, [string] $Hash, [string] $Digest = '', [string] $Signature = '', [string] $DigestBase64 = '')
$ErrorActionPreference = 'Stop'
$bytes = if ($DigestBase64) { [Convert]::FromBase64String($DigestBase64) } else { [System.IO.File]::ReadAllBytes($Digest) }
$text = [System.IO.File]::ReadAllText($Key)
$name = [System.Security.Cryptography.HashAlgorithmName]::new($Hash.ToUpperInvariant())
try {
    $rsa = [System.Security.Cryptography.RSA]::Create()
    $rsa.ImportFromPem($text)
    $out = $rsa.SignHash($bytes, $name, [System.Security.Cryptography.RSASignaturePadding]::Pkcs1)
} catch {
    $ec = [System.Security.Cryptography.ECDsa]::Create()
    $ec.ImportFromPem($text)
    $out = $ec.SignHash($bytes, [System.Security.Cryptography.DSASignatureFormat]::Rfc3279DerSequence)
}
if ($Signature) { [System.IO.File]::WriteAllBytes($Signature, $out) } else { [Console]::Out.Write([Convert]::ToBase64String($out)) }
```

`tools/signing_gate.ps1` signs with this script on every platform, through `{digest}` and `{signature}`, and through `{digest-base64}` with the signature printed.

`pkcs11` loads the token's PKCS#11 library, finds the token by its label and the private key by its label, and signs on the token, logged in with the PIN in the variable `password-env` names.

Each source works on every platform but `store`, which works on Windows and macOS.

## OpenAuthenticode

`signer = "openauthenticode"` signs the scripts and executables with the OpenAuthenticode module, version 0.6.3, which `cargo pwrs` fetches from the PowerShell Gallery the first time and checks against its lock afterwards. It takes the certificate from `pfx`, `pem` or `store`. It takes no other key, so `command` and `pkcs11` are refused with it. It makes no catalogs, so a catalog is made and signed by `cargo pwrs` itself, with the same certificate.

## Checking the signatures

On Windows, with the signer's root certificate trusted:

```powershell
Get-ChildItem target/pwrs/Hello -Recurse -Include *.ps1, *.psm1, *.psd1, *.ps1xml, *.dll, *.exe, *.cat |
    Get-AuthenticodeSignature | Where-Object Status -ne Valid
Test-FileCatalog -Path target/pwrs/Hello -CatalogFilePath target/pwrs/Hello/Hello.cat
pwsh -NoProfile -ExecutionPolicy AllSigned -Command "Import-Module ./target/pwrs/Hello/Hello.psd1"
```

The first prints nothing when every signature is valid. `Test-FileCatalog` prints `Valid`. Importing under `AllSigned` shows no prompt when the signer's certificate is in `TrustedPublisher`. Otherwise PowerShell prompts whether to run software from that publisher.

`tools/signing_gate.ps1` does all of this with test certificates it makes. Its signing half runs on any platform, and its `-Verify` half checks the result with Windows itself, in pwsh and in Windows PowerShell.
