#Requires -Version 7.4
# Checks that cargo pwrs sign signs a module with every key source this
# machine has, through both signers, and that Windows accepts each
# signature.
#
# Signing, the default, runs on any platform. It builds the hello example,
# makes a test root, an RSA and an ECDSA code-signing certificate and a
# timestamping certificate, all in memory and written only under
# target/signing-gate, runs an RFC 3161 timestamp server on a loopback
# port, and signs one copy of the module folder per case under
# target/signing-gate/cases. Each case is then signed again, which must
# leave every file as it is: that checks every signature against its file
# on any platform. Each signed case is imported in a fresh pwsh, which
# calls Get-Greeting. One case changes a file after signing and checks
# that signing again names it. One names its signing command, the
# command's files and the certificate by paths relative to the folder it
# signs from.
#
# -Merge names module folders that other machines built from the same
# commit. They are merged into hello's build before any case is signed,
# as a release folder is, so a case signed on Linux still has the Windows
# runtime -Verify loads. One more case signs hello's own build, merges
# the folders into it, and signs again: only the merged files are signed
# that time, and the catalog is made again.
#
# -Verify runs on Windows and reads the cases a signing run left, on this
# machine or another. Before the test root is trusted, every signature the
# run made reads UnknownError, never HashMismatch. With the root in
# LocalMachine\Root and the signers in LocalMachine\TrustedPublisher, both
# removed again however the run ends, every signature reads Valid in pwsh
# and in Windows PowerShell, every catalog passes Test-FileCatalog, and
# every case imports under the AllSigned execution policy in both hosts.
[CmdletBinding()]
param(
    [switch] $Verify,
    # The cases folder -Verify reads; a signing run's own when empty.
    [string] $Cases = '',
    # A PKCS#11 library, such as SoftHSM2's, and its softhsm2-util, for
    # the pkcs11 case; the case is left out when they are not given.
    [string] $Pkcs11Library = '',
    [string] $SoftHsmUtil = '',
    # Signs a case from the system's store as well. On Windows the RSA
    # signer's PFX is imported into CurrentUser\My, and every certificate
    # it put there is removed again however the run ends. On macOS it goes
    # into a keychain made for the run, which is the default keychain
    # until the run ends and is then deleted.
    [switch] $Store,
    # Module folders of hello that other machines built from this commit,
    # merged into this machine's build before signing.
    [string[]] $Merge = @()
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$root = Split-Path -Parent $PSScriptRoot
$gate = Join-Path $root 'target/signing-gate'
if (-not $Cases) { $Cases = Join-Path $gate 'cases' }
$failures = New-Object System.Collections.ArrayList

function Check($what, $expected, $actual) {
    if ($expected -eq $actual) {
        "  ok   {0}: {1}" -f $what, $actual
    } else {
        "  FAIL {0}: expected '{1}', got '{2}'" -f $what, $expected, $actual
        $null = $failures.Add($what)
    }
}

# Runs a native command with its standard error merged, the exit code
# what counts, as Windows PowerShell stops on stderr under `Stop`.
function Invoke-Native([string] $Exe, [string[]] $Arguments) {
    $prior = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $out = & $Exe @Arguments 2>&1 | ForEach-Object { "$_" }
    } finally {
        $ErrorActionPreference = $prior
    }
    [pscustomobject]@{ Exit = $LASTEXITCODE; Lines = @($out) }
}

# Runs macOS's security tool and returns its output; a failure stops the run.
function Invoke-Security([string[]] $Arguments) {
    $run = Invoke-Native 'security' $Arguments
    if ($run.Exit -ne 0) { throw "security $($Arguments[0]) exited $($run.Exit): $($run.Lines -join ' ')" }
    $run.Lines
}

$scriptExtensions = '.ps1', '.psm1', '.psd1', '.ps1xml', '.psc1', '.cdxml', '.mof'
$binaryExtensions = '.dll', '.exe'

if (-not $Verify) {
    $Merge = @($Merge | ForEach-Object {
        $full = (Resolve-Path -LiteralPath $_).Path
        if (-not (Test-Path (Join-Path $full 'Hello.psd1'))) { throw "-Merge $_ is not a build of hello: it has no Hello.psd1" }
        $full
    })

    # The certificates.
    $fixtures = Join-Path $gate 'fixtures'
    if (Test-Path $gate) { Remove-Item $gate -Recurse -Force }
    $null = New-Item -ItemType Directory -Force -Path $fixtures
    $now = [DateTimeOffset]::UtcNow
    function New-Serial { [byte[]](1..16 | ForEach-Object { Get-Random -Maximum 256 }) }
    function Add-Usage($Request, [string] $Eku, [bool] $Critical) {
        $Request.CertificateExtensions.Add([System.Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new($false, $false, 0, $true))
        $Request.CertificateExtensions.Add([System.Security.Cryptography.X509Certificates.X509KeyUsageExtension]::new('DigitalSignature', $true))
        $oids = [System.Security.Cryptography.OidCollection]::new()
        $null = $oids.Add([System.Security.Cryptography.Oid]::new($Eku))
        $Request.CertificateExtensions.Add([System.Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]::new($oids, $Critical))
    }
    $rootKey = [System.Security.Cryptography.RSA]::Create(3072)
    $rootRequest = [System.Security.Cryptography.X509Certificates.CertificateRequest]::new('CN=PWRS Signing Gate Root', $rootKey, 'SHA256', [System.Security.Cryptography.RSASignaturePadding]::Pkcs1)
    $rootRequest.CertificateExtensions.Add([System.Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new($true, $false, 0, $true))
    $rootRequest.CertificateExtensions.Add([System.Security.Cryptography.X509Certificates.X509KeyUsageExtension]::new('KeyCertSign, CrlSign', $true))
    $rootCert = $rootRequest.CreateSelfSigned($now.AddDays(-1), $now.AddDays(30))
    $rsaKey = [System.Security.Cryptography.RSA]::Create(3072)
    $rsaRequest = [System.Security.Cryptography.X509Certificates.CertificateRequest]::new('CN=PWRS Signing Gate RSA', $rsaKey, 'SHA256', [System.Security.Cryptography.RSASignaturePadding]::Pkcs1)
    Add-Usage $rsaRequest '1.3.6.1.5.5.7.3.3' $false
    $rsaCert = [System.Security.Cryptography.X509Certificates.RSACertificateExtensions]::CopyWithPrivateKey($rsaRequest.Create($rootCert, $now.AddDays(-1), $now.AddDays(29), (New-Serial)), $rsaKey)
    $ecKey = [System.Security.Cryptography.ECDsa]::Create([System.Security.Cryptography.ECCurve+NamedCurves]::nistP256)
    $ecRequest = [System.Security.Cryptography.X509Certificates.CertificateRequest]::new('CN=PWRS Signing Gate ECDSA', $ecKey, 'SHA256')
    Add-Usage $ecRequest '1.3.6.1.5.5.7.3.3' $false
    $byRoot = [System.Security.Cryptography.X509Certificates.X509SignatureGenerator]::CreateForRSA($rootKey, [System.Security.Cryptography.RSASignaturePadding]::Pkcs1)
    $ecCert = [System.Security.Cryptography.X509Certificates.ECDsaCertificateExtensions]::CopyWithPrivateKey($ecRequest.Create($rootCert.SubjectName, $byRoot, $now.AddDays(-1), $now.AddDays(29), (New-Serial)), $ecKey)
    $tsaKey = [System.Security.Cryptography.RSA]::Create(3072)
    $tsaRequest = [System.Security.Cryptography.X509Certificates.CertificateRequest]::new('CN=PWRS Signing Gate Timestamps', $tsaKey, 'SHA256', [System.Security.Cryptography.RSASignaturePadding]::Pkcs1)
    Add-Usage $tsaRequest '1.3.6.1.5.5.7.3.8' $true
    $tsaCert = [System.Security.Cryptography.X509Certificates.RSACertificateExtensions]::CopyWithPrivateKey($tsaRequest.Create($rootCert, $now.AddDays(-1), $now.AddDays(29), (New-Serial)), $tsaKey)

    $secret = 'pwrs-signing-gate'
    $env:PWRS_SIGNING_GATE_SECRET = $secret
    $pem = { param($cert) $cert.ExportCertificatePem() + "`n" }
    $pbe = [System.Security.Cryptography.PbeParameters]::new('Aes256Cbc', [System.Security.Cryptography.HashAlgorithmName]::SHA256, 100000)
    foreach ($signer in @(@{ Name = 'rsa'; Cert = $rsaCert; Key = $rsaKey }, @{ Name = 'ecdsa'; Cert = $ecCert; Key = $ecKey })) {
        $both = [System.Security.Cryptography.X509Certificates.X509Certificate2Collection]::new()
        $null = $both.Add($signer.Cert)
        $null = $both.Add([System.Security.Cryptography.X509Certificates.X509Certificate2]::new($rootCert.RawData))
        [System.IO.File]::WriteAllBytes((Join-Path $fixtures "$($signer.Name).pfx"), $both.Export('Pkcs12', $secret))
        [System.IO.File]::WriteAllText((Join-Path $fixtures "$($signer.Name).pem"), (& $pem $signer.Cert) + (& $pem $rootCert))
        [System.IO.File]::WriteAllText((Join-Path $fixtures "$($signer.Name).key"), $signer.Key.ExportPkcs8PrivateKeyPem() + "`n")
        [System.IO.File]::WriteAllText((Join-Path $fixtures "$($signer.Name)-encrypted.key"), $signer.Key.ExportEncryptedPkcs8PrivateKeyPem($secret, $pbe) + "`n")
        [System.IO.File]::WriteAllBytes((Join-Path $fixtures "$($signer.Name).cer"), $signer.Cert.RawData)
    }
    $null = New-Item -ItemType Directory -Force -Path $Cases
    [System.IO.File]::WriteAllBytes((Join-Path $Cases 'root.cer'), $rootCert.RawData)
    Copy-Item (Join-Path $fixtures 'rsa.cer') (Join-Path $Cases 'rsa.cer')
    Copy-Item (Join-Path $fixtures 'ecdsa.cer') (Join-Path $Cases 'ecdsa.cer')

    # The command a command case runs: it signs the digest it is given
    # with a key from a PEM file, as a key service's tool would, writing
    # the signature to a file or printing it as base64.
    $signDigest = Join-Path $fixtures 'sign-digest.ps1'
    Set-Content -LiteralPath $signDigest -Encoding utf8BOM -Value @'
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
'@
    $pwsh = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
    # A launcher that runs the script above, for the case that names its
    # program by a relative path: a .cmd file on Windows, an executable
    # shell script elsewhere.
    if ($IsWindows) {
        $launcher = 'sign-digest.cmd'
        Set-Content -LiteralPath (Join-Path $fixtures $launcher) -Encoding ascii -Value "@`"$pwsh`" -NoProfile -NonInteractive -File `"%~dp0sign-digest.ps1`" %*"
    } else {
        $launcher = 'sign-digest.sh'
        $launcherPath = Join-Path $fixtures $launcher
        Set-Content -LiteralPath $launcherPath -Encoding ascii -Value "#!/bin/sh`nexec `"$pwsh`" -NoProfile -NonInteractive -File `"`$(dirname `"`$0`")/sign-digest.ps1`" `"`$@`""
        [System.IO.File]::SetUnixFileMode($launcherPath, 'UserRead, UserWrite, UserExecute')
    }

    # Imports a signed case in a fresh pwsh, which calls Get-Greeting.
    function Test-SignedImport([string] $Name, [string] $Folder) {
        $import = Invoke-Native $pwsh @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command', "Import-Module '$Folder/Hello.psd1'; (Get-Greeting -Name signed).ToString()")
        Check "${Name}: imports here once signed" 0 $import.Exit
        if ($import.Exit -ne 0) { $import.Lines | ForEach-Object { "    $_" } }
    }

    # The timestamp server: returns a token for each request, over its
    # imprint and nonce, signed by the timestamping certificate, until a
    # request for /stop. It runs in a second runspace of this process,
    # which every pwsh has, so the certificate passes as the object itself.
    # Windows gives the port, since one the gate picked could be held by
    # another program, as the ports of outgoing connections are. The
    # server sets $listening once it listens and stops at its first error.
    $free = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $free.Start()
    $port = ([System.Net.IPEndPoint]$free.LocalEndpoint).Port
    $free.Stop()
    $tsaUrl = "http://127.0.0.1:$port/"
    $tsaListening = [System.Threading.ManualResetEvent]::new($false)
    $tsaScript = {
        param($url, $cert, $listening)
        $ErrorActionPreference = 'Stop'
        $listener = [System.Net.HttpListener]::new()
        $listener.Prefixes.Add($url)
        $listener.Start()
        $null = $listening.Set()
        $issued = 0
        try {
            while ($true) {
                $context = $listener.GetContext()
                if ($context.Request.Url.AbsolutePath -eq '/stop') {
                    $context.Response.Close()
                    break
                }
                $body = [System.IO.MemoryStream]::new()
                $context.Request.InputStream.CopyTo($body)
                $req = [System.Formats.Asn1.AsnReader]::new([System.ReadOnlyMemory[byte]]::new($body.ToArray()), 'DER').ReadSequence()
                $null = $req.ReadInteger()
                $imprint = $req.ReadEncodedValue().ToArray()
                $nonce = $null
                while ($req.HasData) {
                    $tag = $req.PeekTag()
                    if ($tag.TagValue -eq 2 -and $tag.TagClass -eq 'Universal') { $nonce = $req.ReadInteger() } else { $null = $req.ReadEncodedValue() }
                }
                $w = [System.Formats.Asn1.AsnWriter]::new('DER')
                $null = $w.PushSequence()
                $w.WriteInteger(1)
                $w.WriteObjectIdentifier('1.3.6.1.4.1.99999.1')
                $w.WriteEncodedValue($imprint)
                $w.WriteInteger([System.Numerics.BigInteger]::new([byte[]](1..8 | ForEach-Object { Get-Random -Maximum 128 })))
                $w.WriteGeneralizedTime([DateTimeOffset]::UtcNow)
                if ($null -ne $nonce) { $w.WriteInteger($nonce) }
                $w.PopSequence()
                $content = [System.Security.Cryptography.Pkcs.ContentInfo]::new([System.Security.Cryptography.Oid]::new('1.2.840.113549.1.9.16.1.4'), $w.Encode())
                $cms = [System.Security.Cryptography.Pkcs.SignedCms]::new($content, $false)
                $signer = [System.Security.Cryptography.Pkcs.CmsSigner]::new($cert)
                $signer.DigestAlgorithm = [System.Security.Cryptography.Oid]::new('2.16.840.1.101.3.4.2.1')
                $signer.IncludeOption = 'EndCertOnly'
                $ess = [System.Formats.Asn1.AsnWriter]::new('DER')
                $null = $ess.PushSequence(); $null = $ess.PushSequence(); $null = $ess.PushSequence()
                $ess.WriteOctetString([System.Security.Cryptography.SHA256]::HashData($cert.RawData))
                $ess.PopSequence(); $ess.PopSequence(); $ess.PopSequence()
                $null = $signer.SignedAttributes.Add([System.Security.Cryptography.AsnEncodedData]::new('1.2.840.113549.1.9.16.2.47', $ess.Encode()))
                $cms.ComputeSignature($signer, $true)
                $resp = [System.Formats.Asn1.AsnWriter]::new('DER')
                $null = $resp.PushSequence()
                $null = $resp.PushSequence(); $resp.WriteInteger(0); $resp.PopSequence()
                $resp.WriteEncodedValue($cms.Encode())
                $resp.PopSequence()
                $bytes = $resp.Encode()
                $context.Response.ContentType = 'application/timestamp-reply'
                $context.Response.ContentLength64 = $bytes.Length
                $context.Response.OutputStream.Write($bytes, 0, $bytes.Length)
                $context.Response.OutputStream.Close()
                $context.Response.Close()
                $issued++
            }
        } finally {
            $listener.Stop()
        }
        "issued $issued timestamps"
    }
    $tsa = [powershell]::Create()
    $null = $tsa.AddScript($tsaScript.ToString()).AddArgument($tsaUrl).AddArgument($tsaCert).AddArgument($tsaListening)
    $tsaRun = $tsa.BeginInvoke()
    # The server listens, or its runspace ends first with the reason, and
    # no case runs without it.
    if ([System.Threading.WaitHandle]::WaitAny([System.Threading.WaitHandle[]]@($tsaListening, $tsaRun.AsyncWaitHandle)) -ne 0) {
        $why = $tsa.InvocationStateInfo.Reason
        $tsa.Dispose()
        throw "the timestamp server did not start on $tsaUrl`: $why"
    }

    $storeImported = @()
    $keychain = Join-Path $gate 'signing-gate.keychain-db'
    $keychainMade = $false
    $priorDefault = ''
    try {
        "== building cargo-pwrs and hello"
        $built = Invoke-Native 'cargo' @('build', '--manifest-path', (Join-Path $root 'Cargo.toml'), '--profile', 'test-fast', '-p', 'cargo-pwrs')
        if ($built.Exit -ne 0) { $built.Lines | ForEach-Object { "    $_" }; throw 'cargo-pwrs did not build' }
        $exe = Join-Path $root ('target/test-fast/cargo-pwrs' + $(if ($IsWindows) { '.exe' } else { '' }))
        $hello = Join-Path $root 'examples/hello'
        $build = Invoke-Native $exe @('pwrs', 'build', '--manifest-dir', $hello)
        if ($build.Exit -ne 0) { $build.Lines | ForEach-Object { "    $_" }; throw 'hello did not build' }
        $own = Join-Path $root 'target/pwrs/Hello'
        if (-not (Test-Path (Join-Path $own 'Hello.psd1'))) { throw "the hello build left no $own" }
        $module = $own
        if ($Merge.Count -gt 0) {
            "== merging $($Merge -join ', ')"
            $module = Join-Path $gate 'merged/Hello'
            $null = New-Item -ItemType Directory -Force -Path (Split-Path $module)
            Copy-Item $own -Destination $module -Recurse
            $merged = Invoke-Native $exe (@('pwrs', 'merge', $module) + $Merge)
            $merged.Lines | ForEach-Object { "    $_" }
            if ($merged.Exit -ne 0) { throw 'the -Merge folders did not merge into the hello build' }
        }

        $f = { param($name) Join-Path $fixtures $name }
        $json = { param($value) ConvertTo-Json -Compress -InputObject $value }
        $command = { param($signer, [string[]] $form) & $json (@($pwsh, '-NoProfile', '-NonInteractive', '-File', $signDigest, '-Key', (& $f "$signer.key"), '-Hash', '{hash}') + $form) }
        $caseList = [System.Collections.Generic.List[object]]::new()
        $caseList.Add(@{ Name = 'pfx-rsa'; Args = @('--pfx', (& $f 'rsa.pfx'), '--password-env', 'PWRS_SIGNING_GATE_SECRET', '--timestamp-url', $tsaUrl, '--catalog') })
        $caseList.Add(@{ Name = 'pfx-ecdsa-sha384'; Args = @('--pfx', (& $f 'ecdsa.pfx'), '--password-env', 'PWRS_SIGNING_GATE_SECRET', '--hash', 'sha384') })
        $caseList.Add(@{ Name = 'pem-rsa-sha512'; Args = @('--pem', (& $json @((& $f 'rsa.pem'), (& $f 'rsa-encrypted.key'))), '--password-env', 'PWRS_SIGNING_GATE_SECRET', '--hash', 'sha512', '--catalog') })
        $caseList.Add(@{ Name = 'pem-ecdsa'; Args = @('--pem', (& $json @((& $f 'ecdsa.pem'), (& $f 'ecdsa.key'))), '--timestamp-url', $tsaUrl) })
        $caseList.Add(@{ Name = 'command-files-rsa'; Args = @('--command', (& $command 'rsa' @('-Digest', '{digest}', '-Signature', '{signature}')), '--certificate', (& $f 'rsa.pem'), '--catalog') })
        $caseList.Add(@{ Name = 'command-base64-ecdsa'; Args = @('--command', (& $command 'ecdsa' @('-DigestBase64', '{digest-base64}')), '--certificate', (& $f 'ecdsa.pem'), '--timestamp-url', $tsaUrl) })
        # Signed from the gate's folder, which the program, the key and the
        # certificate are named from.
        $caseList.Add(@{ Name = 'command-relative-rsa'; In = $gate; Args = @('--command', (& $json @("fixtures/$launcher", '-Key', 'fixtures/rsa.key', '-Hash', '{hash}', '-Digest', '{digest}', '-Signature', '{signature}')), '--certificate', 'fixtures/rsa.pem') })
        $caseList.Add(@{ Name = 'openauthenticode-pfx'; Args = @('--pfx', (& $f 'rsa.pfx'), '--password-env', 'PWRS_SIGNING_GATE_SECRET', '--signer', 'openauthenticode', '--timestamp-url', $tsaUrl, '--catalog') })
        if ($Store) {
            if ($IsWindows) {
                $imported = @(Import-PfxCertificate -FilePath (& $f 'rsa.pfx') -CertStoreLocation Cert:\CurrentUser\My -Password (ConvertTo-SecureString $secret -AsPlainText -Force))
                $storeImported = @($imported | ForEach-Object Thumbprint)
                $storeThumbprint = @($imported | Where-Object HasPrivateKey)[0].Thumbprint
            } elseif ($IsMacOS) {
                # CurrentUser\My is the default keychain on macOS. The signer
                # is added to it from this pwsh, so the pwsh that cargo pwrs
                # sign starts, the same program, may use its key.
                $priorDefault = @(Invoke-Security @('default-keychain', '-d', 'user'))[0].Trim().Trim('"')
                $null = Invoke-Security @('create-keychain', '-p', $secret, $keychain)
                $keychainMade = $true
                $null = Invoke-Security @('set-keychain-settings', $keychain)
                $null = Invoke-Security @('unlock-keychain', '-p', $secret, $keychain)
                $null = Invoke-Security @('default-keychain', '-d', 'user', '-s', $keychain)
                $withKey = [System.Security.Cryptography.X509Certificates.X509Certificate2]::new((& $f 'rsa.pfx'), $secret, 'Exportable')
                $my = [System.Security.Cryptography.X509Certificates.X509Store]::new('My', 'CurrentUser')
                $my.Open('ReadWrite')
                try { $my.Add($withKey) } finally { $my.Dispose() }
                $storeThumbprint = $withKey.Thumbprint
            } else {
                throw '-Store signs from the Windows store or the macOS keychain, and this system has neither'
            }
            $caseList.Add(@{ Name = 'store-rsa'; Args = @('--store', $storeThumbprint, '--catalog') })
        }
        if ($Pkcs11Library) {
            $tokens = Join-Path $gate 'softhsm-tokens'
            $null = New-Item -ItemType Directory -Force -Path $tokens
            $config = Join-Path $gate 'softhsm2.conf'
            Set-Content -LiteralPath $config -Value "directories.tokendir = $tokens`nobjectstore.backend = file`n"
            $env:SOFTHSM2_CONF = $config
            $env:PWRS_SIGNING_GATE_PIN = '1234'
            # softhsm2-util works through the library the cases sign with,
            # and its output is printed when it fails.
            $util = {
                param([string] $What, [string[]] $Arguments)
                $run = Invoke-Native $SoftHsmUtil (@('--module', $Pkcs11Library) + $Arguments)
                Check $What 0 $run.Exit
                if ($run.Exit -ne 0) { $run.Lines | ForEach-Object { "    $_" } }
            }
            & $util 'softhsm2-util makes the token' @('--init-token', '--free', '--label', 'pwrs-gate', '--pin', '1234', '--so-pin', '5678')
            & $util 'softhsm2-util imports the RSA key' @('--import', (& $f 'rsa.key'), '--token', 'pwrs-gate', '--label', 'gate-rsa', '--id', '01', '--pin', '1234')
            & $util 'softhsm2-util imports the ECDSA key' @('--import', (& $f 'ecdsa.key'), '--token', 'pwrs-gate', '--label', 'gate-ecdsa', '--id', '02', '--pin', '1234')
            $token = { param($key) '{ library = ' + (& $json $Pkcs11Library) + ', token = "pwrs-gate", key = "' + $key + '" }' }
            $caseList.Add(@{ Name = 'pkcs11-rsa'; Args = @('--pkcs11', (& $token 'gate-rsa'), '--certificate', (& $f 'rsa.pem'), '--password-env', 'PWRS_SIGNING_GATE_PIN', '--catalog', '--timestamp-url', $tsaUrl) })
            $caseList.Add(@{ Name = 'pkcs11-ecdsa'; Args = @('--pkcs11', (& $token 'gate-ecdsa'), '--certificate', (& $f 'ecdsa.pem'), '--password-env', 'PWRS_SIGNING_GATE_PIN') })
        }

        foreach ($case in $caseList) {
            "== $($case.Name)"
            $copy = Join-Path $Cases "$($case.Name)/Hello"
            $null = New-Item -ItemType Directory -Force -Path (Split-Path $copy)
            Copy-Item $module -Destination $copy -Recurse
            $signArgs = @('pwrs', 'sign', '--folder', $copy, '--manifest-dir', $hello) + $case.Args
            if ($case.In) { Push-Location -LiteralPath $case.In }
            try {
                $first = Invoke-Native $exe $signArgs
                $again = Invoke-Native $exe $signArgs
            } finally {
                if ($case.In) { Pop-Location }
            }
            $first.Lines | ForEach-Object { "    $_" }
            Check "$($case.Name): signs" 0 $first.Exit
            $signed = @($first.Lines | Where-Object { $_ -match '^pwrs: signed ' }).Count
            $files = @(Get-ChildItem -LiteralPath $copy -Recurse -File | Where-Object { ($scriptExtensions + $binaryExtensions) -contains $_.Extension.ToLowerInvariant() }).Count
            $leftAlone = @($first.Lines | Where-Object { $_ -match '^pwrs: left .* as it is: it is signed already' }).Count
            Check "$($case.Name): every script and executable signed or left with its own signature" $files ($signed + $leftAlone)
            Check "$($case.Name): signing again exits 0" 0 $again.Exit
            Check "$($case.Name): signing again signs nothing" 0 @($again.Lines | Where-Object { $_ -match '^pwrs: signed ' }).Count
            Test-SignedImport $case.Name $copy
        }

        "== a file changed after signing"
        $changed = Join-Path $Cases 'changed/Hello'
        $null = New-Item -ItemType Directory -Force -Path (Split-Path $changed)
        Copy-Item (Join-Path $Cases 'pfx-rsa/Hello') -Destination $changed -Recurse
        Add-Content -LiteralPath (Join-Path $changed 'Hello.psm1') -Value '# changed after signing'
        $resign = Invoke-Native $exe (@('pwrs', 'sign', '--folder', $changed, '--manifest-dir', $hello) + $caseList[0].Args)
        $resign.Lines | ForEach-Object { "    $_" }
        Check 'the changed file is signed again and named' 1 @($resign.Lines | Where-Object { $_ -match '^pwrs: signed Hello\.psm1 again: its signature does not match its contents' }).Count
        Check 'the catalog is made again' 1 @($resign.Lines | Where-Object { $_ -match '^pwrs: made the catalog Hello\.cat again: Hello\.psm1 has changed' }).Count
        Test-SignedImport 'changed' $changed

        if ($Merge.Count -gt 0) {
            "== merged after signing"
            $late = Join-Path $Cases 'merged-after-signing/Hello'
            $null = New-Item -ItemType Directory -Force -Path (Split-Path $late)
            Copy-Item $own -Destination $late -Recurse
            $lateArgs = @('pwrs', 'sign', '--folder', $late, '--manifest-dir', $hello) + $caseList[0].Args
            $relative = { @(Get-ChildItem -LiteralPath $late -Recurse -File | ForEach-Object { [System.IO.Path]::GetRelativePath($late, $_.FullName).Replace('\', '/') }) }
            $first = Invoke-Native $exe $lateArgs
            Check 'merged after signing: signs the unmerged build' 0 $first.Exit
            if ($first.Exit -ne 0) { $first.Lines | ForEach-Object { "    $_" } }
            $before = & $relative
            $merged = Invoke-Native $exe (@('pwrs', 'merge', $late) + $Merge)
            $merged.Lines | ForEach-Object { "    $_" }
            Check 'merged after signing: the signed folder takes the merged runtimes' 0 $merged.Exit
            $added = @(& $relative | Where-Object { $before -notcontains $_ })
            Check 'merged after signing: the merge adds files' $true ($added.Count -gt 0)
            $again = Invoke-Native $exe $lateArgs
            $again.Lines | ForEach-Object { "    $_" }
            Check 'merged after signing: signs again' 0 $again.Exit
            $signedNames = @($again.Lines | ForEach-Object { if ($_ -match '^pwrs: signed (.+)$') { $Matches[1] } })
            $leftNames = @($again.Lines | ForEach-Object { if ($_ -match '^pwrs: left (.+) as it is: it is signed already') { $Matches[1] } })
            Check 'merged after signing: only merged files are signed' '' (@($signedNames | Where-Object { $added -notcontains $_ }) -join '; ')
            $signable = @($added | Where-Object { ($scriptExtensions + $binaryExtensions) -contains [System.IO.Path]::GetExtension($_).ToLowerInvariant() })
            Check 'merged after signing: every merged script and executable is signed' '' (@($signable | Where-Object { $signedNames -notcontains $_ -and $leftNames -notcontains $_ }) -join '; ')
            Check 'merged after signing: the catalog is made again' 1 @($again.Lines | Where-Object { $_ -match '^pwrs: made the catalog Hello\.cat again: it does not list ' }).Count
            $third = Invoke-Native $exe $lateArgs
            Check 'merged after signing: signing a third time exits 0' 0 $third.Exit
            if ($third.Exit -ne 0) { $third.Lines | ForEach-Object { "    $_" } }
            Check 'merged after signing: signing a third time signs nothing' 0 @($third.Lines | Where-Object { $_ -match '^pwrs: signed ' }).Count
            Test-SignedImport 'merged after signing' $late
        }
    } finally {
        foreach ($print in $storeImported) { Remove-Item -LiteralPath "Cert:\CurrentUser\My\$print" }
        if ($keychainMade) {
            if ($priorDefault) { $null = Invoke-Native 'security' @('default-keychain', '-d', 'user', '-s', $priorDefault) }
            $null = Invoke-Native 'security' @('delete-keychain', $keychain)
        }
        # A server that stopped at an error has nothing listening for /stop.
        if ($tsa.InvocationStateInfo.State -eq 'Running') { $null = Invoke-WebRequest -Uri "$($tsaUrl)stop" -UseBasicParsing }
        try {
            $tsa.EndInvoke($tsaRun) | ForEach-Object { "  timestamp server: $_" }
        } catch {
            "  timestamp server stopped at an error: $($_.Exception.Message)"
        }
        $tsa.Streams.Error | ForEach-Object { "  timestamp server error: $_" }
        $tsa.Dispose()
    }
} else {
    if (-not $IsWindows) { throw '-Verify checks the signatures with Windows itself, and this is not Windows' }
    $rootFile = Join-Path $Cases 'root.cer'
    if (-not (Test-Path $rootFile)) { throw "$Cases has no root.cer; run the signing half first" }
    $rootCert = [System.Security.Cryptography.X509Certificates.X509Certificate2]::new($rootFile)
    $signers = @('rsa.cer', 'ecdsa.cer' | ForEach-Object { [System.Security.Cryptography.X509Certificates.X509Certificate2]::new((Join-Path $Cases $_)) })
    $signerPrints = @($signers | ForEach-Object Thumbprint)
    $caseDirs = @(Get-ChildItem -LiteralPath $Cases -Directory | Where-Object { Test-Path (Join-Path $_.FullName 'Hello/Hello.psd1') })

    # Each signed file's status in one host, as "<path> <status> <signer thumbprint>" lines.
    $probe = {
        param($Folder)
        Get-ChildItem -LiteralPath $Folder -Recurse -File | Where-Object { '.ps1', '.psm1', '.psd1', '.ps1xml', '.psc1', '.cdxml', '.mof', '.dll', '.exe', '.cat' -contains $_.Extension.ToLowerInvariant() } | ForEach-Object {
            $s = Get-AuthenticodeSignature -LiteralPath $_.FullName
            '{0}|{1}|{2}|{3}' -f $_.FullName.Substring($Folder.Length + 1), $s.Status, $s.SignerCertificate.Thumbprint, [bool]$s.TimeStamperCertificate
        }
    }
    function Get-Statuses([string] $Folder, [string] $Host_) {
        $encoded = [Convert]::ToBase64String([System.Text.Encoding]::Unicode.GetBytes("& { $probe } '$Folder'"))
        $run = Invoke-Native $Host_ @('-NoProfile', '-NonInteractive', '-EncodedCommand', $encoded)
        if ($run.Exit -ne 0) { throw "$Host_ could not read the signatures in ${Folder}: $($run.Lines -join ' ')" }
        foreach ($line in $run.Lines) {
            $p = $line -split '\|'
            if ($p.Count -eq 4) { [pscustomobject]@{ Path = $p[0]; Status = $p[1]; Signer = $p[2]; Timestamped = $p[3] -eq 'True' } }
        }
    }

    "== before the test root is trusted"
    foreach ($dir in $caseDirs) {
        $folder = Join-Path $dir.FullName 'Hello'
        foreach ($s in Get-Statuses $folder 'pwsh') {
            if ($signerPrints -contains $s.Signer) { Check "$($dir.Name)/$($s.Path)" 'UnknownError' $s.Status }
        }
    }

    $trusted = @()
    try {
        $null = Import-Certificate -FilePath $rootFile -CertStoreLocation Cert:\LocalMachine\Root
        $trusted += "Cert:\LocalMachine\Root\$($rootCert.Thumbprint)"
        foreach ($name in 'rsa.cer', 'ecdsa.cer') {
            $c = Import-Certificate -FilePath (Join-Path $Cases $name) -CertStoreLocation Cert:\LocalMachine\TrustedPublisher
            $trusted += "Cert:\LocalMachine\TrustedPublisher\$($c.Thumbprint)"
        }
        foreach ($dir in $caseDirs) {
            $folder = Join-Path $dir.FullName 'Hello'
            "== $($dir.Name)"
            foreach ($hostExe in 'pwsh', 'powershell') {
                $statuses = @(Get-Statuses $folder $hostExe)
                $bad = @($statuses | Where-Object Status -ne 'Valid' | ForEach-Object { "$($_.Path) $($_.Status)" })
                Check "$($dir.Name) in ${hostExe}: every signature Valid" '' ($bad -join '; ')
                Check "$($dir.Name) in ${hostExe}: files signed by the run" $true (@($statuses | Where-Object { $signerPrints -contains $_.Signer }).Count -gt 0)
            }
            $catalog = Join-Path $folder 'Hello.cat'
            if (Test-Path $catalog) {
                $result = Test-FileCatalog -Path $folder -CatalogFilePath $catalog -Detailed
                Check "$($dir.Name): Test-FileCatalog" 'Valid' "$($result.Status)"
                Check "$($dir.Name): the catalog's signature" 'Valid' "$($result.Signature.Status)"
            }
            foreach ($hostExe in 'pwsh', 'powershell') {
                $import = Invoke-Native $hostExe @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'AllSigned', '-Command', "Import-Module '$folder\Hello.psd1'; (Get-Greeting -Name signed).ToString()")
                Check "$($dir.Name) in ${hostExe}: imports under AllSigned" 0 $import.Exit
                if ($import.Exit -ne 0) { $import.Lines | ForEach-Object { "    $_" } }
            }
        }
    } finally {
        foreach ($item in $trusted) { Remove-Item -LiteralPath $item }
    }
}

if ($failures.Count -gt 0) {
    "signing gate: $($failures.Count) failed"
    exit 1
}
'signing gate: all passed'
