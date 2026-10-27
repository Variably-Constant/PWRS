using System;
using System.Collections.Generic;
using System.Formats.Asn1;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text;

namespace Pwrs.Signing
{
    /// <summary>
    /// A file catalog as New-FileCatalog -CatalogVersion 2 writes one: a
    /// certificate trust list naming every file of a folder by its path
    /// and hashes, signed as a whole.
    /// </summary>
    internal static class Catalog
    {
        const string SubjectUsageOid = "1.3.6.1.4.1.311.12.1.1";
        const string MemberV2Oid = "1.3.6.1.4.1.311.12.1.3";
        const string MemberInfoOid = "1.3.6.1.4.1.311.12.2.3";
        const string NameValueOid = "1.3.6.1.4.1.311.12.2.1";
        const string FilePath = "FilePath";
        const int NameValueFlags = 0x10010001;

        /// <summary>
        /// Every file under <paramref name="folder"/> but the catalog at
        /// <paramref name="catalog"/>, by its path relative to the folder
        /// with backslashes, as Test-FileCatalog names them, in order.
        /// </summary>
        internal static List<(string Relative, string Full)> FilesOf(string folder, string catalog)
        {
            string root = Path.GetFullPath(folder);
            return Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories)
                .Where(f => !string.Equals(Path.GetFullPath(f), Path.GetFullPath(catalog), StringComparison.OrdinalIgnoreCase))
                .Select(f => (Relative: Path.GetRelativePath(root, f).Replace('/', '\\'), Full: f))
                .OrderBy(f => f.Relative, StringComparer.Ordinal)
                .ToList();
        }

        /// <summary>
        /// The SHA256 and SHA1 a catalog lists a non-empty file under: an
        /// executable's Authenticode digests, which leave out its
        /// signature, and any other file's hashes of its bytes; and which
        /// of the two the file is.
        /// </summary>
        static (byte[] Sha256, byte[] Sha1, bool Image) KeysOf(string full, string relative, byte[] bytes)
        {
            byte[]? image256 = Pe.ImageDigest(full, relative, HashAlgorithmName.SHA256);
            if (image256 == null) return (SHA256.HashData(bytes), SHA1.HashData(bytes), false);
            return (image256, Pe.ImageDigest(full, relative, HashAlgorithmName.SHA1) ?? throw new SigningException($"{relative} stopped being a Windows executable while it was hashed"), true);
        }

        /// <summary>
        /// The catalog's list: two entries for each file, under its SHA256
        /// with its path and indirect data and under its SHA1 with its
        /// path, sorted by hash, and each empty file, which has no hash to
        /// list, named in the list's own attributes.
        /// </summary>
        internal static byte[] List(List<(string Relative, string Full)> files)
        {
            var entries = new List<(byte[] Key, byte[] Der)>();
            var empty = new List<string>();
            foreach (var (relative, full) in files)
            {
                byte[] bytes = File.ReadAllBytes(full);
                if (bytes.Length == 0)
                {
                    empty.Add(relative);
                    continue;
                }
                var (sha256, sha1, image) = KeysOf(full, relative, bytes);
                byte[] indirect = image
                    ? Cms.IndirectData(Cms.PeImageDataOid, ImageData(), HashAlgorithmName.SHA256, sha256)
                    : Cms.IndirectData(Cms.CabDataOid, CabData(), HashAlgorithmName.SHA256, sha256);
                int member = image ? 0 : 2;
                entries.Add((sha256, Entry(sha256, relative, member, indirect)));
                entries.Add((sha1, Entry(sha1, relative, member, null)));
            }
            entries.Sort((a, b) => a.Key.AsSpan().SequenceCompareTo(b.Key));
            var w = new AsnWriter(AsnEncodingRules.DER);
            using (w.PushSequence())
            {
                using (w.PushSequence()) w.WriteObjectIdentifier(SubjectUsageOid);
                w.WriteOctetString(RandomNumberGenerator.GetBytes(16));
                w.WriteUtcTime(DateTimeOffset.UtcNow);
                using (w.PushSequence())
                {
                    w.WriteObjectIdentifier(MemberV2Oid);
                    w.WriteNull();
                }
                using (w.PushSequence())
                {
                    foreach (var (_, der) in entries) w.WriteEncodedValue(der);
                }
                if (empty.Count > 0)
                {
                    using (w.PushSequence(new Asn1Tag(TagClass.ContextSpecific, 0, true)))
                    using (w.PushSequence())
                    {
                        foreach (string name in empty)
                        {
                            using (w.PushSequence())
                            {
                                w.WriteObjectIdentifier(NameValueOid);
                                w.WriteOctetString(PathValue(name));
                            }
                        }
                    }
                }
            }
            return w.Encode();
        }

        /// <summary>
        /// One entry: the key, then the member information, whose empty
        /// value is tagged <paramref name="member"/> (0 for an executable,
        /// 2 for any other file), the path, and the indirect data when
        /// given.
        /// </summary>
        static byte[] Entry(byte[] key, string relative, int member, byte[]? indirect)
        {
            var w = new AsnWriter(AsnEncodingRules.DER);
            using (w.PushSequence())
            {
                w.WriteOctetString(key);
                using (w.PushSetOf())
                {
                    using (w.PushSequence())
                    {
                        w.WriteObjectIdentifier(MemberInfoOid);
                        using (w.PushSetOf()) w.WriteOctetString(ReadOnlySpan<byte>.Empty, new Asn1Tag(TagClass.ContextSpecific, member));
                    }
                    using (w.PushSequence())
                    {
                        w.WriteObjectIdentifier(NameValueOid);
                        using (w.PushSetOf()) w.WriteEncodedValue(PathValue(relative));
                    }
                    if (indirect != null)
                    {
                        using (w.PushSequence())
                        {
                            w.WriteObjectIdentifier(Cms.IndirectDataOid);
                            using (w.PushSetOf()) w.WriteEncodedValue(indirect);
                        }
                    }
                }
            }
            return w.Encode();
        }

        /// <summary>
        /// SPC_PE_IMAGE_DATA as it appears in a catalog: the resources and
        /// import address table flags set, and an empty link.
        /// </summary>
        static byte[] ImageData()
        {
            var w = new AsnWriter(AsnEncodingRules.DER);
            using (w.PushSequence())
            {
                w.WriteBitString(new byte[] { 0xA0 }, 5);
                using (w.PushSequence(new Asn1Tag(TagClass.ContextSpecific, 0, true)))
                using (w.PushSequence(new Asn1Tag(TagClass.ContextSpecific, 2, true)))
                {
                    w.WriteOctetString(ReadOnlySpan<byte>.Empty, new Asn1Tag(TagClass.ContextSpecific, 0));
                }
            }
            return w.Encode();
        }

        /// <summary>The name-value pair that names a file: "FilePath", its flags, and the path as UTF-16LE with a NUL.</summary>
        static byte[] PathValue(string relative)
        {
            var w = new AsnWriter(AsnEncodingRules.DER);
            using (w.PushSequence())
            {
                w.WriteCharacterString(UniversalTagNumber.BMPString, FilePath);
                w.WriteInteger(NameValueFlags);
                w.WriteOctetString(Encoding.Unicode.GetBytes(relative + "\0"));
            }
            return w.Encode();
        }

        /// <summary>SPC_CAB_DATA's empty link, as it appears in a catalog.</summary>
        static byte[] CabData()
        {
            var w = new AsnWriter(AsnEncodingRules.DER);
            using (w.PushSequence(new Asn1Tag(TagClass.ContextSpecific, 2, true)))
            {
                w.WriteOctetString(ReadOnlySpan<byte>.Empty, new Asn1Tag(TagClass.ContextSpecific, 0));
            }
            return w.Encode();
        }

        /// <summary>
        /// Why the catalog at <paramref name="path"/> does not match
        /// <paramref name="files"/>, null when it does: its signature
        /// checks and it lists exactly these files with their present
        /// SHA256 hashes.
        /// </summary>
        internal static string? Fault(string path, List<(string Relative, string Full)> files, out string signer)
        {
            signer = "";
            var (cms, fault) = Cms.Check(File.ReadAllBytes(path));
            if (cms == null) return fault;
            signer = Cms.Signer(cms);
            if (cms.ContentInfo.ContentType.Value != Cms.CtlOid) return "it is not a catalog";
            var listed = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            var emptyListed = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            try
            {
                var ctl = new AsnReader(cms.ContentInfo.Content, AsnEncodingRules.BER).ReadSequence();
                ctl.ReadSequence();
                ctl.ReadOctetString();
                ctl.ReadEncodedValue();
                ctl.ReadSequence();
                var entries = ctl.ReadSequence();
                while (entries.HasData)
                {
                    var entry = entries.ReadSequence();
                    byte[] key = entry.ReadOctetString();
                    string? name = null;
                    var attributes = entry.ReadSetOf();
                    while (attributes.HasData)
                    {
                        var attribute = attributes.ReadSequence();
                        string oid = attribute.ReadObjectIdentifier();
                        var values = attribute.ReadSetOf();
                        if (oid == NameValueOid) name = ReadPath(values.ReadEncodedValue());
                    }
                    if (name != null && key.Length == 32) listed[name] = Convert.ToHexString(key);
                }
                if (ctl.HasData)
                {
                    var extensions = ctl.ReadSequence(new Asn1Tag(TagClass.ContextSpecific, 0, true)).ReadSequence();
                    while (extensions.HasData)
                    {
                        var extension = extensions.ReadSequence();
                        if (extension.ReadObjectIdentifier() == NameValueOid)
                        {
                            string? name = ReadPath(extension.ReadOctetString());
                            if (name != null) emptyListed.Add(name);
                        }
                    }
                }
            }
            catch (AsnContentException e)
            {
                return $"its list cannot be read: {e.Message}";
            }
            foreach (var (relative, full) in files)
            {
                byte[] bytes = File.ReadAllBytes(full);
                if (bytes.Length == 0)
                {
                    if (!emptyListed.Remove(relative)) return $"it does not list {relative}";
                    continue;
                }
                if (!listed.Remove(relative, out string? hash)) return $"it does not list {relative}";
                if (hash != Convert.ToHexString(KeysOf(full, relative, bytes).Sha256)) return $"{relative} has changed since it was made";
            }
            string? gone = listed.Keys.Concat(emptyListed).FirstOrDefault();
            return gone == null ? null : $"it lists {gone}, which the folder no longer has";
        }

        static string? ReadPath(ReadOnlyMemory<byte> value)
        {
            var pair = new AsnReader(value, AsnEncodingRules.BER).ReadSequence();
            if (pair.ReadCharacterString(UniversalTagNumber.BMPString) != FilePath) return null;
            pair.ReadInteger();
            return Encoding.Unicode.GetString(pair.ReadOctetString()).TrimEnd('\0');
        }
    }
}
