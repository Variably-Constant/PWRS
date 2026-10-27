using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.Formats.Asn1;
using System.IO;
using System.Linq;
using System.Security.Cryptography;

namespace Pwrs.Signing
{
    /// <summary>
    /// Signatures on Windows executables: an Authenticode digest of the
    /// image, signed in a CMS signature stored as the image's certificate
    /// table.
    /// </summary>
    internal static class Pe
    {
        internal static readonly string[] Extensions = { ".dll", ".exe" };

        const ushort Revision2 = 0x0200;
        const ushort PkcsSignedData = 0x0002;

        /// <summary>
        /// An image's bytes and where the parts the digest leaves out are:
        /// the checksum, the certificate table's directory entry, and the
        /// certificate table itself.
        /// </summary>
        internal sealed class Image
        {
            internal byte[] Bytes = Array.Empty<byte>();
            internal int ChecksumAt;
            internal int TableEntryAt;
            internal int SizeOfHeaders;
            internal List<(int Pointer, int Size)> Sections = new List<(int, int)>();
            internal int TableAt;
            internal int TableSize;
        }

        /// <summary>The image in the file, or null when the file is not a Windows executable.</summary>
        internal static Image? Read(string path, string name)
        {
            byte[] b = File.ReadAllBytes(path);
            if (b.Length < 0x40 || b[0] != (byte)'M' || b[1] != (byte)'Z') return null;
            int pe = BinaryPrimitives.ReadInt32LittleEndian(b.AsSpan(0x3C));
            if (pe <= 0 || pe > b.Length - 24 || BinaryPrimitives.ReadUInt32LittleEndian(b.AsSpan(pe)) != 0x00004550) return null;
            int sections = BinaryPrimitives.ReadUInt16LittleEndian(b.AsSpan(pe + 6));
            int optionalSize = BinaryPrimitives.ReadUInt16LittleEndian(b.AsSpan(pe + 20));
            int opt = pe + 24;
            if (opt + optionalSize > b.Length) throw new SigningException($"cannot sign {name}: its optional header runs past the end of the file");
            ushort magic = BinaryPrimitives.ReadUInt16LittleEndian(b.AsSpan(opt));
            bool plus = magic == 0x20B;
            if (!plus && magic != 0x10B) throw new SigningException($"cannot sign {name}: its optional header has the magic 0x{magic:X}, which is neither PE32 nor PE32+");
            int directories = opt + (plus ? 112 : 96);
            int rvaCount = BinaryPrimitives.ReadInt32LittleEndian(b.AsSpan(opt + (plus ? 108 : 92)));
            if (rvaCount < 5) throw new SigningException($"cannot sign {name}: its header has no certificate table entry");
            var image = new Image
            {
                Bytes = b,
                ChecksumAt = opt + 64,
                TableEntryAt = directories + 4 * 8,
                SizeOfHeaders = BinaryPrimitives.ReadInt32LittleEndian(b.AsSpan(opt + 60)),
                TableAt = BinaryPrimitives.ReadInt32LittleEndian(b.AsSpan(directories + 4 * 8)),
                TableSize = BinaryPrimitives.ReadInt32LittleEndian(b.AsSpan(directories + 4 * 8 + 4)),
            };
            int table = opt + optionalSize;
            for (int i = 0; i < sections; i++)
            {
                int at = table + i * 40;
                if (at + 40 > b.Length) throw new SigningException($"cannot sign {name}: its section table runs past the end of the file");
                int size = BinaryPrimitives.ReadInt32LittleEndian(b.AsSpan(at + 16));
                int pointer = BinaryPrimitives.ReadInt32LittleEndian(b.AsSpan(at + 20));
                if (size > 0) image.Sections.Add((pointer, size));
            }
            if (image.TableAt != 0 && (image.TableAt < 0 || image.TableSize < 8 || (long)image.TableAt + image.TableSize > b.Length))
            {
                throw new SigningException($"cannot sign {name}: its certificate table entry points outside the file");
            }
            return image;
        }

        /// <summary>
        /// The Authenticode digest: the headers without the checksum and
        /// the certificate table entry, each section in file order, and the
        /// data after the sections, without the certificate table.
        /// </summary>
        static byte[] Digest(Image image, HashAlgorithmName hash, int length)
        {
            byte[] b = image.Bytes;
            using var h = IncrementalHash.CreateHash(hash);
            h.AppendData(b, 0, image.ChecksumAt);
            h.AppendData(b, image.ChecksumAt + 4, image.TableEntryAt - (image.ChecksumAt + 4));
            h.AppendData(b, image.TableEntryAt + 8, image.SizeOfHeaders - (image.TableEntryAt + 8));
            long hashed = image.SizeOfHeaders;
            foreach (var (pointer, size) in image.Sections.OrderBy(s => s.Pointer))
            {
                if ((long)pointer + size > length) throw new SigningException("a section runs past the end of the file");
                h.AppendData(b, pointer, size);
                hashed += size;
            }
            long table = image.TableAt != 0 ? image.TableSize : 0;
            long extra = length - table - hashed;
            if (extra > 0) h.AppendData(b, (int)hashed, (int)extra);
            return h.GetHashAndReset();
        }

        /// <summary>
        /// The Authenticode digest of the image in the file, its
        /// certificate table left out, which is what a catalog lists an
        /// executable under; null when the file is not a Windows executable.
        /// </summary>
        internal static byte[]? ImageDigest(string path, string name, HashAlgorithmName hash)
        {
            var image = Read(path, name);
            return image == null ? null : Digest(image, hash, image.Bytes.Length);
        }

        /// <summary>
        /// Why the signature in the image's certificate table is not valid,
        /// null when it is. The image must have a table.
        /// </summary>
        internal static string? Fault(Image image, out string signer)
        {
            signer = "";
            byte[] b = image.Bytes;
            int length = BinaryPrimitives.ReadInt32LittleEndian(b.AsSpan(image.TableAt));
            ushort revision = BinaryPrimitives.ReadUInt16LittleEndian(b.AsSpan(image.TableAt + 4));
            ushort type = BinaryPrimitives.ReadUInt16LittleEndian(b.AsSpan(image.TableAt + 6));
            if (type != PkcsSignedData || length < 8 || length > image.TableSize) return $"its certificate table has a certificate of type {type}, revision 0x{revision:X}, which is not a signature";
            byte[] der;
            try
            {
                AsnDecoder.ReadEncodedValue(b.AsSpan(image.TableAt + 8, length - 8), AsnEncodingRules.BER, out _, out _, out int consumed);
                der = b.AsSpan(image.TableAt + 8, consumed).ToArray();
            }
            catch (AsnContentException e)
            {
                return $"its signature cannot be read: {e.Message}";
            }
            var (cms, fault) = Cms.Check(der);
            if (cms == null) return fault;
            signer = Cms.Signer(cms);
            if (cms.ContentInfo.ContentType.Value != Cms.IndirectDataOid) return "its signature is not an Authenticode signature";
            try
            {
                var (dataType, hash, digest) = Cms.ReadIndirectData(cms.ContentInfo.Content);
                if (dataType != Cms.PeImageDataOid) return "its signature is not an executable's signature";
                if (!Digest(image, hash, b.Length).AsSpan().SequenceEqual(digest)) return "its signature does not match its contents";
            }
            catch (Exception e) when (e is CryptographicException || e is AsnContentException)
            {
                return $"its signature's content cannot be read: {e.Message}";
            }
            return null;
        }

        /// <summary>
        /// Signs the image: an old certificate table removed, the file
        /// padded to eight bytes, the signature appended as the new table,
        /// the directory entry pointed at it, and the checksum recomputed.
        /// </summary>
        internal static void Sign(string path, string name, Image image, Key key, HashAlgorithmName hash, string? timestampUrl)
        {
            int length = image.Bytes.Length;
            if (image.TableAt != 0)
            {
                if (image.TableAt + image.TableSize < length - 7)
                {
                    throw new SigningException($"cannot sign {name} again: data follows its certificate table, and removing the table would cut that data off");
                }
                length = image.TableAt;
            }
            int padded = (length + 7) & ~7;
            var b = new byte[padded];
            Buffer.BlockCopy(image.Bytes, 0, b, 0, length);
            BinaryPrimitives.WriteInt64LittleEndian(b.AsSpan(image.TableEntryAt), 0);
            var unsigned = new Image
            {
                Bytes = b,
                ChecksumAt = image.ChecksumAt,
                TableEntryAt = image.TableEntryAt,
                SizeOfHeaders = image.SizeOfHeaders,
                Sections = image.Sections,
            };
            byte[] digest = Digest(unsigned, hash, padded);
            byte[] indirect = Cms.IndirectData(Cms.PeImageDataOid, Cms.PeImageData(), hash, digest);
            byte[] signature = Cms.Sign(Cms.IndirectDataOid, indirect, true, key, hash, timestampUrl);
            int entry = (8 + signature.Length + 7) & ~7;
            var signed = new byte[padded + entry];
            Buffer.BlockCopy(b, 0, signed, 0, padded);
            BinaryPrimitives.WriteInt32LittleEndian(signed.AsSpan(padded), entry);
            BinaryPrimitives.WriteUInt16LittleEndian(signed.AsSpan(padded + 4), Revision2);
            BinaryPrimitives.WriteUInt16LittleEndian(signed.AsSpan(padded + 6), PkcsSignedData);
            Buffer.BlockCopy(signature, 0, signed, padded + 8, signature.Length);
            BinaryPrimitives.WriteInt32LittleEndian(signed.AsSpan(image.TableEntryAt), padded);
            BinaryPrimitives.WriteInt32LittleEndian(signed.AsSpan(image.TableEntryAt + 4), entry);
            BinaryPrimitives.WriteUInt32LittleEndian(signed.AsSpan(image.ChecksumAt), Checksum(signed, image.ChecksumAt));
            Files.Replace(path, signed);
        }

        /// <summary>
        /// The PE checksum: the file summed as 16-bit words with the carry
        /// folded back in, the checksum field left out, plus the length.
        /// </summary>
        static uint Checksum(byte[] b, int checksumAt)
        {
            ulong sum = 0;
            for (int i = 0; i + 1 < b.Length; i += 2)
            {
                if (i == checksumAt || i == checksumAt + 2) continue;
                sum += BinaryPrimitives.ReadUInt16LittleEndian(b.AsSpan(i));
                sum = (sum & 0xFFFF) + (sum >> 16);
            }
            if ((b.Length & 1) != 0)
            {
                sum += b[b.Length - 1];
                sum = (sum & 0xFFFF) + (sum >> 16);
            }
            sum = (sum & 0xFFFF) + (sum >> 16);
            return (uint)sum + (uint)b.Length;
        }
    }
}
