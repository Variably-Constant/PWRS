using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Formats.Asn1;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Security.Cryptography.X509Certificates;

namespace Pwrs.Signing
{
    /// <summary>
    /// The signer's certificate, the other certificates a signature includes
    /// with it, and a private key that signs a digest, from one of the key
    /// sources the settings name.
    /// </summary>
    public sealed class Key : IDisposable
    {
        const string CodeSigningOid = "1.3.6.1.5.5.7.3.3";

        internal X509Certificate2 Certificate { get; }
        internal X509Certificate2Collection Chain { get; }
        internal AsymmetricAlgorithm PrivateKey { get; }

        /// <summary>
        /// The certificate with its key, for a signer that takes one: set
        /// for pfx, pem and store, null for command and pkcs11.
        /// </summary>
        public X509Certificate2? WithKey { get; }

        readonly IDisposable? _token;

        Key(X509Certificate2 certificate, X509Certificate2Collection chain, AsymmetricAlgorithm privateKey, X509Certificate2? withKey, IDisposable? token)
        {
            Certificate = certificate;
            Chain = chain;
            PrivateKey = privateKey;
            WithKey = withKey;
            _token = token;
        }

        public void Dispose() => _token?.Dispose();

        /// <summary>The signer's subject, for what the run reports.</summary>
        public string Subject => Certificate.Subject;

        internal static Key Load(Settings s)
        {
            string? secret = null;
            if (s.PasswordEnv != null)
            {
                secret = Environment.GetEnvironmentVariable(s.PasswordEnv);
                if (secret == null)
                {
                    throw new SigningException($"password-env names {s.PasswordEnv}, and no variable of that name is set");
                }
            }
            return s.Source switch
            {
                "pfx" => FromPfx(s.Pfx!, secret),
                "pem" => FromPem(s.PemCertificate!, s.PemKey!, secret),
                "store" => FromStore(s.Store!),
                "command" => FromCommand(s.Command!, s.CommandDir ?? throw new SigningException("the settings name no folder for the command to run in"), s.Certificate!),
                "pkcs11" => FromToken(s, secret),
                _ => throw new SigningException($"no key source {s.Source}"),
            };
        }

        static Key FromPfx(string path, string? password)
        {
            // macOS has no ephemeral keys; a PFX key there is stored in a
            // temporary keychain while the certificate is in use.
            var flags = OperatingSystem.IsMacOS() ? X509KeyStorageFlags.DefaultKeySet : X509KeyStorageFlags.EphemeralKeySet;
            var all = new X509Certificate2Collection();
            try
            {
                all.Import(path, password, flags);
            }
            catch (CryptographicException e)
            {
                throw new SigningException(password == null
                    ? $"{path} cannot be read without a password ({e.Message}); set password-env to the name of a variable set to the password"
                    : $"{path} cannot be read with the password in the variable password-env names: {e.Message}");
            }
            var leaf = Choose(all.Where(c => c.HasPrivateKey).ToList(), path);
            return new Key(leaf, Others(all, leaf), PrivateKeyOf(leaf, path), leaf, null);
        }

        static Key FromPem(string certificatePath, string keyPath, string? password)
        {
            var all = new X509Certificate2Collection();
            all.ImportFromPemFile(certificatePath);
            if (all.Count == 0)
            {
                throw new SigningException($"{certificatePath} has no certificate");
            }
            string keyText = File.ReadAllText(keyPath);
            AsymmetricAlgorithm key = ImportPemKey(keyText, password, keyPath);
            byte[] publicKey = key switch
            {
                RSA rsa => rsa.ExportSubjectPublicKeyInfo(),
                ECDsa ec => ec.ExportSubjectPublicKeyInfo(),
                _ => throw new SigningException($"{keyPath} has a key that is neither RSA nor ECDSA"),
            };
            var leaf = all.Cast<X509Certificate2>().FirstOrDefault(c => PublicKeyOf(c)?.SequenceEqual(publicKey) == true)
                ?? throw new SigningException($"no certificate in {certificatePath} is the certificate of the key in {keyPath}");
            var withKey = key is RSA r ? leaf.CopyWithPrivateKey(r) : leaf.CopyWithPrivateKey((ECDsa)key);
            return new Key(withKey, Others(all, leaf), key, withKey, null);
        }

        static AsymmetricAlgorithm ImportPemKey(string text, string? password, string path)
        {
            var failures = new List<string>();
            foreach (var make in new Func<AsymmetricAlgorithm>[] { () => RSA.Create(), () => ECDsa.Create() })
            {
                var key = make();
                try
                {
                    if (password == null)
                    {
                        if (key is RSA rsa) rsa.ImportFromPem(text); else ((ECDsa)key).ImportFromPem(text);
                    }
                    else
                    {
                        if (key is RSA rsa) rsa.ImportFromEncryptedPem(text, password); else ((ECDsa)key).ImportFromEncryptedPem(text, password);
                    }
                    return key;
                }
                catch (Exception e) when (e is CryptographicException || e is ArgumentException)
                {
                    failures.Add(e.Message);
                    key.Dispose();
                }
            }
            throw new SigningException($"{path} has no RSA or ECDSA private key that can be read{(password == null ? " without a password; set password-env if it is encrypted" : " with the password password-env names")}: {string.Join("; ", failures)}");
        }

        static Key FromStore(string thumbprint)
        {
            if (!OperatingSystem.IsWindows() && !OperatingSystem.IsMacOS())
            {
                throw new SigningException("store reads the Windows certificate store or the macOS keychain, and this system has neither; give pfx, pem, command or pkcs11");
            }
            string wanted = new string(thumbprint.Where(char.IsLetterOrDigit).ToArray()).ToUpperInvariant();
            var places = OperatingSystem.IsWindows() ? new[] { StoreLocation.CurrentUser, StoreLocation.LocalMachine } : new[] { StoreLocation.CurrentUser };
            foreach (var place in places)
            {
                using var store = new X509Store(StoreName.My, place);
                store.Open(OpenFlags.ReadOnly | OpenFlags.OpenExistingOnly);
                foreach (var c in store.Certificates.Find(X509FindType.FindByThumbprint, wanted, false))
                {
                    if (!c.HasPrivateKey) continue;
                    var chain = new X509Certificate2Collection();
                    using (var built = new X509Chain())
                    {
                        built.ChainPolicy.RevocationMode = X509RevocationMode.NoCheck;
                        built.ChainPolicy.VerificationFlags = X509VerificationFlags.AllowUnknownCertificateAuthority;
                        built.Build(c);
                        foreach (var element in built.ChainElements.Cast<X509ChainElement>().Skip(1)) chain.Add(element.Certificate);
                    }
                    return new Key(c, chain, PrivateKeyOf(c, $"the certificate {wanted}"), c, null);
                }
            }
            throw new SigningException(OperatingSystem.IsWindows()
                ? $"neither CurrentUser\\My nor LocalMachine\\My has the certificate {wanted} with its private key"
                : $"the keychain has no certificate {wanted} with its private key");
        }

        static Key FromCommand(string[] command, string dir, string certificatePath)
        {
            var (leaf, chain) = ReadCertificates(certificatePath);
            var runner = new SigningCommand(command, dir);
            return new Key(leaf, chain, External(leaf, certificatePath, runner.Sign, $"the signing command {command[0]}", true), null, null);
        }

        static Key FromToken(Settings s, string? pin)
        {
            if (pin == null)
            {
                throw new SigningException("pkcs11 needs the token's PIN: set password-env to the name of a variable set to the PIN");
            }
            var (leaf, chain) = ReadCertificates(s.Certificate!);
            bool ec = leaf.GetECDsaPublicKey() != null;
            var token = new Pkcs11Token(s.Pkcs11Library!, s.Pkcs11Token!, s.Pkcs11Key!, pin, ec);
            try
            {
                return new Key(leaf, chain, External(leaf, s.Certificate!, token.Sign, $"the key {s.Pkcs11Key} on the token {s.Pkcs11Token}", false), null, token);
            }
            catch
            {
                token.Dispose();
                throw;
            }
        }

        /// <summary>
        /// A key that signs through <paramref name="sign"/>, of the kind
        /// the certificate's public key is. An ECDSA signature arrives as
        /// DER when <paramref name="der"/>, else as r and s side by side.
        /// </summary>
        static AsymmetricAlgorithm External(X509Certificate2 leaf, string path, Func<byte[], HashAlgorithmName, byte[]> sign, string what, bool der)
        {
            if (leaf.GetRSAPublicKey() is RSA rsa) return new ExternalRsa(rsa, sign, what);
            if (leaf.GetECDsaPublicKey() is ECDsa ec) return new ExternalEcdsa(ec, sign, what, der);
            throw new SigningException($"the certificate in {path} has a key that is neither RSA nor ECDSA");
        }

        /// <summary>
        /// The certificates in a PEM, DER or PKCS#7 file, and the one among
        /// them that issued none of the others, which is the signer's.
        /// </summary>
        static (X509Certificate2 Leaf, X509Certificate2Collection Chain) ReadCertificates(string path)
        {
            var all = new X509Certificate2Collection();
            byte[] bytes = File.ReadAllBytes(path);
            try
            {
                if (System.Text.Encoding.ASCII.GetString(bytes, 0, Math.Min(bytes.Length, 64)).Contains("-----BEGIN")) all.ImportFromPemFile(path);
                else all.Import(bytes);
            }
            catch (CryptographicException e)
            {
                throw new SigningException($"{path} cannot be read as certificates in PEM, DER or PKCS#7: {e.Message}");
            }
            var list = all.Cast<X509Certificate2>().ToList();
            var leaf = list.FirstOrDefault(c => !list.Any(o => !ReferenceEquals(o, c) && o.IssuerName.RawData.SequenceEqual(c.SubjectName.RawData)))
                ?? throw new SigningException($"{path} has no certificate");
            return (leaf, Others(all, leaf));
        }

        /// <summary>
        /// The one certificate of <paramref name="withKeys"/> to sign
        /// with: the only one, or the only one for code signing.
        /// </summary>
        static X509Certificate2 Choose(List<X509Certificate2> withKeys, string path)
        {
            if (withKeys.Count == 1) return withKeys[0];
            if (withKeys.Count == 0) throw new SigningException($"{path} has no certificate with its private key");
            var signing = withKeys.Where(IsForCodeSigning).ToList();
            if (signing.Count == 1) return signing[0];
            throw new SigningException($"{path} has {withKeys.Count} certificates with private keys, {signing.Count} of them for code signing, and one is needed: {string.Join("; ", withKeys.Select(c => c.Subject))}");
        }

        static bool IsForCodeSigning(X509Certificate2 c) =>
            c.Extensions.OfType<X509EnhancedKeyUsageExtension>().Any(e => e.EnhancedKeyUsages.Cast<Oid>().Any(o => o.Value == CodeSigningOid));

        static X509Certificate2Collection Others(X509Certificate2Collection all, X509Certificate2 leaf)
        {
            var others = new X509Certificate2Collection();
            foreach (var c in all)
            {
                if (!c.RawData.SequenceEqual(leaf.RawData)) others.Add(c);
            }
            return others;
        }

        static byte[]? PublicKeyOf(X509Certificate2 c) =>
            (c.GetRSAPublicKey() as AsymmetricAlgorithm ?? c.GetECDsaPublicKey()) switch
            {
                RSA rsa => rsa.ExportSubjectPublicKeyInfo(),
                ECDsa ec => ec.ExportSubjectPublicKeyInfo(),
                _ => null,
            };

        static AsymmetricAlgorithm PrivateKeyOf(X509Certificate2 c, string what) =>
            (AsymmetricAlgorithm?)c.GetRSAPrivateKey() ?? c.GetECDsaPrivateKey()
            ?? throw new SigningException($"{what} has a key that is neither RSA nor ECDSA");
    }

    /// <summary>
    /// An RSA key whose signatures come from elsewhere, each checked
    /// against the certificate's public key before it is used.
    /// </summary>
    internal sealed class ExternalRsa : RSA
    {
        readonly RSA _public;
        readonly Func<byte[], HashAlgorithmName, byte[]> _sign;
        readonly string _what;

        internal ExternalRsa(RSA publicKey, Func<byte[], HashAlgorithmName, byte[]> sign, string what)
        {
            _public = publicKey;
            _sign = sign;
            _what = what;
            KeySizeValue = publicKey.KeySize;
            LegalKeySizesValue = new[] { new KeySizes(publicKey.KeySize, publicKey.KeySize, 0) };
        }

        public override byte[] SignHash(byte[] hash, HashAlgorithmName hashAlgorithm, RSASignaturePadding padding)
        {
            if (padding != RSASignaturePadding.Pkcs1)
            {
                throw new CryptographicException($"Authenticode signs with PKCS#1 v1.5 padding, not {padding}");
            }
            byte[] signature = _sign(hash, hashAlgorithm);
            if (!_public.VerifyHash(hash, signature, hashAlgorithm, RSASignaturePadding.Pkcs1))
            {
                throw new SigningException($"{_what} gave a signature that does not check against the certificate's public key");
            }
            return signature;
        }

        public override bool VerifyHash(byte[] hash, byte[] signature, HashAlgorithmName hashAlgorithm, RSASignaturePadding padding) =>
            _public.VerifyHash(hash, signature, hashAlgorithm, padding);

        public override RSAParameters ExportParameters(bool includePrivateParameters) =>
            includePrivateParameters ? throw new CryptographicException($"{_what} does not give its private key") : _public.ExportParameters(false);

        public override void ImportParameters(RSAParameters parameters) => throw new NotSupportedException();
    }

    /// <summary>
    /// An ECDSA key whose signatures come from elsewhere, each checked
    /// against the certificate's public key before it is used.
    /// </summary>
    internal sealed class ExternalEcdsa : ECDsa
    {
        readonly ECDsa _public;
        readonly Func<byte[], HashAlgorithmName, byte[]> _sign;
        readonly string _what;
        readonly bool _der;

        internal ExternalEcdsa(ECDsa publicKey, Func<byte[], HashAlgorithmName, byte[]> sign, string what, bool der)
        {
            _public = publicKey;
            _sign = sign;
            _what = what;
            _der = der;
            KeySizeValue = publicKey.KeySize;
            LegalKeySizesValue = new[] { new KeySizes(publicKey.KeySize, publicKey.KeySize, 0) };
        }

        public override byte[] SignHash(byte[] hash)
        {
            var algorithm = hash.Length switch
            {
                32 => HashAlgorithmName.SHA256,
                48 => HashAlgorithmName.SHA384,
                64 => HashAlgorithmName.SHA512,
                _ => throw new CryptographicException($"a {hash.Length}-byte digest is not one Authenticode signs"),
            };
            byte[] signature = _sign(hash, algorithm);
            if (_der) signature = SideBySide(signature, (KeySize + 7) / 8);
            if (!_public.VerifyHash(hash, signature))
            {
                throw new SigningException($"{_what} gave a signature that does not check against the certificate's public key");
            }
            return signature;
        }

        /// <summary>A DER ECDSA signature as r and s, each <paramref name="width"/> bytes.</summary>
        byte[] SideBySide(byte[] der, int width)
        {
            try
            {
                var seq = new AsnReader(der, AsnEncodingRules.DER).ReadSequence();
                byte[] r = seq.ReadIntegerBytes().ToArray();
                byte[] s = seq.ReadIntegerBytes().ToArray();
                var both = new byte[2 * width];
                Place(r, both, 0, width);
                Place(s, both, width, width);
                return both;
            }
            catch (Exception e) when (e is AsnContentException || e is CryptographicException || e is ArgumentException)
            {
                throw new SigningException($"{_what} gave an ECDSA signature that is not DER: {e.Message}");
            }
        }

        static void Place(byte[] value, byte[] into, int at, int width)
        {
            int skip = 0;
            while (skip < value.Length - 1 && value[skip] == 0) skip++;
            int len = value.Length - skip;
            if (len > width) throw new ArgumentException("an integer of the signature is wider than the key");
            Buffer.BlockCopy(value, skip, into, at + width - len, len);
        }

        public override bool VerifyHash(byte[] hash, byte[] signature) => _public.VerifyHash(hash, signature);

        public override ECParameters ExportParameters(bool includePrivateParameters) =>
            includePrivateParameters ? throw new CryptographicException($"{_what} does not give its private key") : _public.ExportParameters(false);

        public override ECParameters ExportExplicitParameters(bool includePrivateParameters) =>
            includePrivateParameters ? throw new CryptographicException($"{_what} does not give its private key") : _public.ExportExplicitParameters(false);

        public override void ImportParameters(ECParameters parameters) => throw new NotSupportedException();
    }

    /// <summary>
    /// The command key source: runs the configured command once per
    /// signature, in the folder its setting came from, with the digest in
    /// the form its arguments name, and reads the signature from the file
    /// it writes or from its output.
    /// </summary>
    internal sealed class SigningCommand
    {
        readonly string[] _command;
        readonly string _dir;

        internal SigningCommand(string[] command, string dir)
        {
            _command = command;
            _dir = dir;
        }

        /// <summary>
        /// The program to start: a relative path, one with a folder in it,
        /// taken from the command's folder; a bare name, found on PATH.
        /// </summary>
        string Program(string program) =>
            !Path.IsPathRooted(program) && program.IndexOfAny(new[] { Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar }) >= 0
                ? Path.GetFullPath(Path.Combine(_dir, program))
                : program;

        internal byte[] Sign(byte[] digest, HashAlgorithmName hash)
        {
            string dir = Directory.CreateTempSubdirectory("pwrs-sign-").FullName;
            try
            {
                string digestFile = Path.Combine(dir, "digest.bin");
                string signatureFile = Path.Combine(dir, "signature.bin");
                File.WriteAllBytes(digestFile, digest);
                string Expand(string arg) => arg
                    .Replace("{digest-base64}", Convert.ToBase64String(digest))
                    .Replace("{digest-hex}", Convert.ToHexString(digest).ToLowerInvariant())
                    .Replace("{digest}", digestFile)
                    .Replace("{signature}", signatureFile)
                    .Replace("{hash}", hash.Name!.ToLowerInvariant());
                var start = new ProcessStartInfo(Program(Expand(_command[0])))
                {
                    UseShellExecute = false,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    WorkingDirectory = _dir,
                };
                foreach (string arg in _command.Skip(1)) start.ArgumentList.Add(Expand(arg));
                string output, error;
                int exit;
                try
                {
                    using var p = Process.Start(start) ?? throw new SigningException($"the signing command {_command[0]} did not start");
                    var errorRead = p.StandardError.ReadToEndAsync();
                    output = p.StandardOutput.ReadToEnd();
                    error = errorRead.GetAwaiter().GetResult();
                    p.WaitForExit();
                    exit = p.ExitCode;
                }
                catch (System.ComponentModel.Win32Exception e)
                {
                    throw new SigningException($"the signing command {_command[0]} cannot be started: {e.Message}");
                }
                if (exit != 0)
                {
                    throw new SigningException($"the signing command {_command[0]} exited with {exit}{(error.Trim().Length > 0 ? ": " + error.Trim() : "")}");
                }
                if (_command.Any(a => a.Contains("{signature}")))
                {
                    if (!File.Exists(signatureFile))
                    {
                        throw new SigningException($"the signing command {_command[0]} exited with 0 and wrote no signature to the file {{signature}} named");
                    }
                    return File.ReadAllBytes(signatureFile);
                }
                string base64 = new string(output.Where(c => !char.IsWhiteSpace(c)).ToArray());
                try
                {
                    return Convert.FromBase64String(base64);
                }
                catch (FormatException)
                {
                    string first = output.Trim().Split('\n').FirstOrDefault()?.Trim() ?? "";
                    throw new SigningException($"the signing command {_command[0]} printed no base64 signature (its output starts: {(first.Length > 80 ? first.Substring(0, 80) : first)}); name {{signature}} in its arguments if it writes the signature to a file");
                }
            }
            finally
            {
                Directory.Delete(dir, true);
            }
        }
    }
}
