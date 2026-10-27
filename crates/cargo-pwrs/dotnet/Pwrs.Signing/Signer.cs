using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Security.Cryptography.X509Certificates;
using System.Text.Json;

namespace Pwrs.Signing
{
    /// <summary>What `cargo pwrs sign` was given, from the settings file it writes.</summary>
    internal sealed class Settings
    {
        internal string Folder = "";
        internal string Module = "";
        internal string Source = "";
        internal string? Pfx, PemCertificate, PemKey, Store, Pkcs11Library, Pkcs11Token, Pkcs11Key;
        internal string[]? Command;
        /// <summary>The folder the command runs in: the one its setting came from.</summary>
        internal string? CommandDir;
        internal string? PasswordEnv, Certificate, TimestampUrl;
        internal HashAlgorithmName Hash = HashAlgorithmName.SHA256;
        internal bool Catalog;
        internal string Signer = "pwrs";

        internal static Settings Parse(string json)
        {
            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            string? Text(string name) => root.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;
            var s = new Settings
            {
                Folder = Text("folder") ?? throw new SigningException("the settings name no folder"),
                Module = Text("module") ?? throw new SigningException("the settings name no module"),
                Source = Text("source") ?? throw new SigningException("the settings name no key source"),
                Pfx = Text("pfx"),
                Store = Text("store"),
                CommandDir = Text("command_dir"),
                PasswordEnv = Text("password_env"),
                Certificate = Text("certificate"),
                TimestampUrl = Text("timestamp_url"),
                Signer = Text("signer") ?? "pwrs",
                Catalog = root.TryGetProperty("catalog", out var c) && c.ValueKind == JsonValueKind.True,
                Hash = (Text("hash") ?? "sha256") switch
                {
                    "sha256" => HashAlgorithmName.SHA256,
                    "sha384" => HashAlgorithmName.SHA384,
                    "sha512" => HashAlgorithmName.SHA512,
                    string other => throw new SigningException($"hash {other} is not sha256, sha384 or sha512"),
                },
            };
            if (root.TryGetProperty("pem", out var pem) && pem.ValueKind == JsonValueKind.Array && pem.GetArrayLength() == 2)
            {
                s.PemCertificate = pem[0].GetString();
                s.PemKey = pem[1].GetString();
            }
            if (root.TryGetProperty("command", out var command) && command.ValueKind == JsonValueKind.Array)
            {
                s.Command = command.EnumerateArray().Select(a => a.GetString() ?? "").ToArray();
            }
            if (root.TryGetProperty("pkcs11", out var token) && token.ValueKind == JsonValueKind.Object)
            {
                s.Pkcs11Library = token.TryGetProperty("library", out var l) ? l.GetString() : null;
                s.Pkcs11Token = token.TryGetProperty("token", out var t) ? t.GetString() : null;
                s.Pkcs11Key = token.TryGetProperty("key", out var k) ? k.GetString() : null;
            }
            return s;
        }
    }

    /// <summary>
    /// One run of `cargo pwrs sign` over a module folder: the key loaded,
    /// every file sorted into those to sign, those left as they are and
    /// those refused, then the signing and the catalog. Every outcome is a
    /// line of the run's report.
    /// </summary>
    public sealed class Plan : IDisposable
    {
        readonly Settings _s;
        readonly Key _key;
        readonly string _catalogPath;

        /// <summary>The files to sign, each by its path within the folder, with why when it is signed again.</summary>
        public List<(string Path, string? Again)> ToSign { get; } = new List<(string, string?)>();

        /// <summary>The lines that need no signing: the files left as they are, and the files with no signature of their own.</summary>
        public List<string> Report { get; } = new List<string>();

        /// <summary>The files that cannot be signed, each line naming the file and what to do.</summary>
        public List<string> Refusals { get; } = new List<string>();

        public string Folder => _s.Folder;
        public string Signer => _s.Signer;
        public string Hash => _s.Hash.Name!;
        public string? TimestampUrl => _s.TimestampUrl;
        public string Subject => _key.Subject;

        Plan(Settings s, Key key)
        {
            _s = s;
            _key = key;
            _catalogPath = System.IO.Path.Combine(s.Folder, s.Module + ".cat");
        }

        /// <summary>Loads the key and sorts the folder's files.</summary>
        public static Plan Prepare(string settingsJson)
        {
            var s = Settings.Parse(settingsJson);
            var key = Key.Load(s);
            var plan = new Plan(s, key);
            try
            {
                plan.Sort();
            }
            catch
            {
                plan.Dispose();
                throw;
            }
            return plan;
        }

        void Sort()
        {
            int others = 0;
            foreach (var (relative, full) in Catalog.FilesOf(_s.Folder, _catalogPath))
            {
                string name = relative.Replace('\\', '/');
                string extension = System.IO.Path.GetExtension(full).ToLowerInvariant();
                try
                {
                    if (Scripts.Extensions.Contains(extension))
                    {
                        var script = Scripts.Read(full, name);
                        if (script.Signature == null) ToSign.Add((name, null));
                        else LeaveOrSignAgain(name, Scripts.Fault(script, out string signer), signer);
                    }
                    else if (Pe.Extensions.Contains(extension))
                    {
                        var image = Pe.Read(full, name);
                        if (image == null)
                        {
                            Report.Add($"left {name} unsigned: it is not a Windows executable, so it takes no signature of its own");
                            others++;
                        }
                        else if (image.TableAt == 0) ToSign.Add((name, null));
                        else LeaveOrSignAgain(name, Pe.Fault(image, out string signer), signer);
                    }
                    else
                    {
                        others++;
                    }
                }
                catch (SigningException e)
                {
                    Refusals.Add(e.Message);
                }
            }
            if (others > 0)
            {
                Report.Add(_s.Catalog
                    ? $"{others} other files take no signature of their own; the catalog covers them"
                    : $"{others} other files take no signature of their own; catalog = true covers them with a signed catalog");
            }
        }

        /// <summary>
        /// A signed file is left as it is when its signature is still valid,
        /// and is signed again, with the reason, when
        /// <paramref name="fault"/> names one.
        /// </summary>
        void LeaveOrSignAgain(string name, string? fault, string signer)
        {
            if (fault == null) Report.Add($"left {name} as it is: it is signed already, by {signer}");
            else ToSign.Add((name, fault));
        }

        string Full(string name) => System.IO.Path.Combine(_s.Folder, name.Replace('/', System.IO.Path.DirectorySeparatorChar));

        /// <summary>Signs one file with the `pwrs` signer and returns its line.</summary>
        public string Sign(string name, string? again)
        {
            string full = Full(name);
            string extension = System.IO.Path.GetExtension(full).ToLowerInvariant();
            if (Scripts.Extensions.Contains(extension))
            {
                Scripts.Sign(full, Scripts.Read(full, name), _key, _s.Hash, _s.TimestampUrl);
            }
            else
            {
                var image = Pe.Read(full, name) ?? throw new SigningException($"cannot sign {name}: it is no longer a Windows executable");
                Pe.Sign(full, name, image, _key, _s.Hash, _s.TimestampUrl);
            }
            return Confirm(name, again);
        }

        /// <summary>
        /// Checks that a file's new signature is valid, and returns its
        /// line.
        /// </summary>
        public string Confirm(string name, string? again)
        {
            string full = Full(name);
            string extension = System.IO.Path.GetExtension(full).ToLowerInvariant();
            string? fault;
            if (Scripts.Extensions.Contains(extension))
            {
                var script = Scripts.Read(full, name);
                fault = script.Signature == null ? "it has no signature" : Scripts.Fault(script, out _);
            }
            else
            {
                var image = Pe.Read(full, name);
                fault = image == null ? "it is not a Windows executable" : image.TableAt == 0 ? "it has no signature" : Pe.Fault(image, out _);
            }
            if (fault != null) throw new SigningException($"{name} was signed, and {fault}");
            return string.IsNullOrEmpty(again) ? $"signed {name}" : $"signed {name} again: {again}";
        }

        /// <summary>
        /// The catalog's line: made and signed over every file of the
        /// folder, or left as it is when it already lists them as they are
        /// and its signature is valid; null when catalog is not set.
        /// </summary>
        public string? MakeCatalog()
        {
            if (!_s.Catalog) return null;
            var files = Catalog.FilesOf(_s.Folder, _catalogPath);
            string name = _s.Module + ".cat";
            string? why = null;
            if (File.Exists(_catalogPath))
            {
                why = Catalog.Fault(_catalogPath, files, out string signer);
                if (why == null) return $"left the catalog {name} as it is: it lists the folder's {files.Count} files as they are, signed by {signer}";
            }
            byte[] signature = Cms.Sign(Cms.CtlOid, Catalog.List(files), false, _key, _s.Hash, _s.TimestampUrl);
            Files.Replace(_catalogPath, signature);
            if (Catalog.Fault(_catalogPath, files, out _) is string fault) throw new SigningException($"the catalog {name} was made, and {fault}");
            return why == null ? $"made the catalog {name} over the folder's {files.Count} files" : $"made the catalog {name} again: {why}";
        }

        /// <summary>The signer's certificate with its key, for OpenAuthenticode.</summary>
        public X509Certificate2 CertificateWithKey() =>
            _key.WithKey ?? throw new SigningException($"OpenAuthenticode signs only with a certificate that has its key, from pfx, pem or store; {_s.Source} gives none");

        public void Dispose() => _key.Dispose();
    }
}
