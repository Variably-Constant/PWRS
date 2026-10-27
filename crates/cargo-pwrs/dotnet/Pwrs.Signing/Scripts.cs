using System;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text;

namespace Pwrs.Signing
{
    /// <summary>
    /// Signatures on PowerShell's text files, in the block at their end
    /// that the PowerShell SIP reads: a digest of the text before the
    /// block, as UTF-16LE, signed in a CMS signature written as base64
    /// lines of comments.
    /// </summary>
    internal static class Scripts
    {
        /// <summary>The extensions the PowerShell SIP signs.</summary>
        internal static readonly string[] Extensions = { ".ps1", ".psm1", ".psd1", ".ps1xml", ".psc1", ".cdxml", ".mof" };

        sealed class Style
        {
            internal readonly string Start, End, Prefix, Suffix;
            internal Style(string start, string end, string prefix, string suffix)
            {
                Start = start;
                End = end;
                Prefix = prefix;
                Suffix = suffix;
            }
        }

        static readonly Style Hash = new Style("# SIG # Begin signature block", "# SIG # End signature block", "# ", "");
        static readonly Style Xml = new Style("<!-- SIG # Begin signature block -->", "<!-- SIG # End signature block -->", "<!-- ", " -->");
        static readonly Style Mof = new Style("/* SIG # Begin signature block */", "/* SIG # End signature block */", "/* ", " */");

        static Style StyleOf(string path) => Path.GetExtension(path).ToLowerInvariant() switch
        {
            ".ps1xml" or ".psc1" or ".cdxml" => Xml,
            ".mof" => Mof,
            _ => Hash,
        };

        /// <summary>
        /// A script as the SIP reads it: its encoding, its text without its
        /// signature block, and the signature in the block. The text
        /// decodes strictly, so encoding it again gives back the file's
        /// bytes.
        /// </summary>
        internal sealed class Script
        {
            internal Encoding Encoding = Encoding.UTF8;
            internal string Content = "";
            internal byte[]? Signature;
        }

        /// <summary>
        /// Reads a script, refusing one whose digest would depend on the
        /// reading machine's code page and one whose text does not decode
        /// cleanly in the encoding the SIP takes for it.
        /// </summary>
        internal static Script Read(string path, string name)
        {
            byte[] bytes = File.ReadAllBytes(path);
            Encoding encoding = EncodingOf(bytes, name);
            string text;
            try
            {
                text = encoding.GetString(bytes);
            }
            catch (DecoderFallbackException)
            {
                throw new SigningException($"cannot sign {name}: Windows reads it as {Describe(encoding)}, and its bytes are not valid {Describe(encoding)} throughout");
            }
            var script = new Script { Encoding = encoding, Content = text };
            var style = StyleOf(path);
            int at = text.LastIndexOf("\r\n" + style.Start, StringComparison.Ordinal);
            if (at < 0) return script;
            string rest = text.Substring(at + 2 + style.Start.Length);
            int end = rest.IndexOf("\r\n" + style.End, StringComparison.Ordinal);
            if (!rest.StartsWith("\r\n", StringComparison.Ordinal) || end < 0) return script;
            // The block ends with its end marker and up to two line breaks.
            // Text after them belongs to the content, before the block a
            // new signature appends.
            string after = rest.Substring(end + 2 + style.End.Length);
            int breaks = 0;
            while (breaks < after.Length && breaks < 2 && (after[breaks] == '\r' || after[breaks] == '\n')) breaks++;
            string following = after.Substring(breaks);
            var base64 = new StringBuilder();
            foreach (string line in rest.Substring(2, Math.Max(0, end - 2)).Split("\r\n"))
            {
                if (line.Length == 0) continue;
                if (!line.StartsWith(style.Prefix, StringComparison.Ordinal) || !line.EndsWith(style.Suffix, StringComparison.Ordinal)) return script;
                base64.Append(line, style.Prefix.Length, line.Length - style.Prefix.Length - style.Suffix.Length);
            }
            try
            {
                script.Signature = Convert.FromBase64String(base64.ToString());
            }
            catch (FormatException)
            {
                script.Signature = Array.Empty<byte>();
            }
            script.Content = text.Substring(0, at) + following;
            return script;
        }

        /// <summary>
        /// The encoding the PowerShell SIP reads a script in: the one its
        /// byte order mark names; UTF-8 when its first 32 bytes contain a
        /// multibyte UTF-8 sequence and no malformed one; otherwise the
        /// reading machine's code page, refused here when the file has a
        /// byte above 127, since no one signature then reads alike on
        /// every machine. A file of ASCII alone reads the same in all.
        /// </summary>
        static Encoding EncodingOf(byte[] b, string name)
        {
            if (b.Length >= 3 && b[0] == 0xEF && b[1] == 0xBB && b[2] == 0xBF) return new UTF8Encoding(true, true);
            if (b.Length >= 2 && b[0] == 0xFF && b[1] == 0xFE) return new UnicodeEncoding(false, true, true);
            if (b.Length >= 2 && b[0] == 0xFE && b[1] == 0xFF) return new UnicodeEncoding(true, true, true);
            if (StartsAsUtf8(b.AsSpan(0, Math.Min(b.Length, 32)))) return new UTF8Encoding(false, true);
            if (b.Any(x => x > 127))
            {
                throw new SigningException($"cannot sign {name}: it has no byte order mark and its first 32 bytes show no UTF-8, so Windows reads it in the code page of whichever machine reads it, and it has text beyond ASCII. Save it as UTF-8 with a byte order mark");
            }
            return new UTF8Encoding(false, true);
        }

        /// <summary>
        /// Whether <paramref name="head"/> contains at least one multibyte
        /// UTF-8 sequence and every sequence in it is whole and well formed.
        /// </summary>
        static bool StartsAsUtf8(ReadOnlySpan<byte> head)
        {
            bool multibyte = false;
            int following = 0;
            foreach (byte b in head)
            {
                if (following > 0)
                {
                    if ((b & 0xC0) != 0x80) return false;
                    following--;
                    continue;
                }
                if (b < 0x80) continue;
                multibyte = true;
                int ones = 0;
                for (int bit = 7; bit >= 0 && (b & (1 << bit)) != 0; bit--) ones++;
                if (ones < 2) return false;
                following = ones - 1;
            }
            return multibyte && following == 0;
        }

        static string Describe(Encoding e) => e switch
        {
            UnicodeEncoding u when u.CodePage == 1201 => "UTF-16 big-endian",
            UnicodeEncoding => "UTF-16",
            _ => "UTF-8",
        };

        /// <summary>
        /// Why the script's signature is not valid, null when it is:
        /// its digest matches the text and its signature checks.
        /// </summary>
        internal static string? Fault(Script s, out string signer)
        {
            signer = "";
            var (cms, fault) = Cms.Check(s.Signature ?? Array.Empty<byte>());
            if (cms == null) return fault;
            signer = Cms.Signer(cms);
            if (cms.ContentInfo.ContentType.Value != Cms.IndirectDataOid) return "its signature is not an Authenticode signature";
            try
            {
                var (type, hash, digest) = Cms.ReadIndirectData(cms.ContentInfo.Content);
                if (type != Cms.SipInfoOid) return "its signature is not a script signature";
                if (!Cms.Hash(hash, Encoding.Unicode.GetBytes(s.Content)).AsSpan().SequenceEqual(digest)) return "its signature does not match its contents";
            }
            catch (Exception e) when (e is CryptographicException || e is System.Formats.Asn1.AsnContentException)
            {
                return $"its signature's content cannot be read: {e.Message}";
            }
            return null;
        }

        /// <summary>
        /// Signs the script at <paramref name="path"/>: its text without
        /// any old block, then the new block, in the file's encoding.
        /// </summary>
        internal static void Sign(string path, Script s, Key key, HashAlgorithmName hash, string? timestampUrl)
        {
            byte[] digest = Cms.Hash(hash, Encoding.Unicode.GetBytes(s.Content));
            byte[] indirect = Cms.IndirectData(Cms.SipInfoOid, Cms.PowerShellSipInfo(), hash, digest);
            byte[] signature = Cms.Sign(Cms.IndirectDataOid, indirect, true, key, hash, timestampUrl);
            var style = StyleOf(path);
            var block = new StringBuilder();
            block.Append("\r\n").Append(style.Start).Append("\r\n");
            string base64 = Convert.ToBase64String(signature);
            for (int i = 0; i < base64.Length; i += 64)
            {
                block.Append(style.Prefix).Append(base64, i, Math.Min(64, base64.Length - i)).Append(style.Suffix).Append("\r\n");
            }
            block.Append(style.End).Append("\r\n");
            Files.Replace(path, s.Encoding.GetBytes(s.Content + block));
        }
    }

    /// <summary>Writing a file whole.</summary>
    internal static class Files
    {
        /// <summary>
        /// Replaces the file at <paramref name="path"/> with
        /// <paramref name="bytes"/>, written beside it first and renamed
        /// over it, so the file is either the old one or the new one.
        /// </summary>
        internal static void Replace(string path, byte[] bytes)
        {
            string partial = path + ".pwrs-signing";
            File.WriteAllBytes(partial, bytes);
            File.Move(partial, path, true);
        }
    }
}
