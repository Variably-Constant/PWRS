using System;
using System.Formats.Asn1;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Security.Cryptography;
using System.Security.Cryptography.Pkcs;
using System.Security.Cryptography.X509Certificates;

namespace Pwrs.Signing
{
    /// <summary>
    /// The structures every Authenticode signature shares: the indirect
    /// data a signature covers, the CMS signature over it, and the RFC 3161
    /// timestamp on that signature.
    /// </summary>
    internal static class Cms
    {
        internal const string IndirectDataOid = "1.3.6.1.4.1.311.2.1.4";
        internal const string SipInfoOid = "1.3.6.1.4.1.311.2.1.30";
        internal const string PeImageDataOid = "1.3.6.1.4.1.311.2.1.15";
        internal const string CabDataOid = "1.3.6.1.4.1.311.2.1.25";
        internal const string CtlOid = "1.3.6.1.4.1.311.10.1";
        const string OpusInfoOid = "1.3.6.1.4.1.311.2.1.12";
        const string StatementTypeOid = "1.3.6.1.4.1.311.2.1.11";
        const string IndividualPurposeOid = "1.3.6.1.4.1.311.2.1.21";
        const string Rfc3161CounterSignOid = "1.3.6.1.4.1.311.3.3.1";

        /// <summary>The PowerShell SIP's GUID, 603BCC1F-4B59-4E08-B724-D2C6297EF351.</summary>
        static readonly Guid PowerShellSip = new Guid("603BCC1F-4B59-4E08-B724-D2C6297EF351");

        static readonly HttpClient Http = new HttpClient();

        internal static string HashOid(HashAlgorithmName hash) => hash.Name switch
        {
            "SHA1" => "1.3.14.3.2.26",
            "SHA256" => "2.16.840.1.101.3.4.2.1",
            "SHA384" => "2.16.840.1.101.3.4.2.2",
            "SHA512" => "2.16.840.1.101.3.4.2.3",
            _ => throw new ArgumentException($"no digest algorithm {hash.Name}"),
        };

        internal static HashAlgorithmName HashFromOid(string? oid) => oid switch
        {
            "1.3.14.3.2.26" => HashAlgorithmName.SHA1,
            "2.16.840.1.101.3.4.2.1" => HashAlgorithmName.SHA256,
            "2.16.840.1.101.3.4.2.2" => HashAlgorithmName.SHA384,
            "2.16.840.1.101.3.4.2.3" => HashAlgorithmName.SHA512,
            _ => throw new CryptographicException($"the digest algorithm {oid} is not one Authenticode uses"),
        };

        internal static byte[] Hash(HashAlgorithmName hash, ReadOnlySpan<byte> data) => hash.Name switch
        {
            "SHA1" => SHA1.HashData(data),
            "SHA256" => SHA256.HashData(data),
            "SHA384" => SHA384.HashData(data),
            "SHA512" => SHA512.HashData(data),
            _ => throw new ArgumentException($"no digest algorithm {hash.Name}"),
        };

        /// <summary>A DigestInfo: the algorithm, with NULL parameters, and the digest.</summary>
        internal static void WriteDigestInfo(AsnWriter w, HashAlgorithmName hash, byte[] digest)
        {
            using (w.PushSequence())
            {
                using (w.PushSequence())
                {
                    w.WriteObjectIdentifier(HashOid(hash));
                    w.WriteNull();
                }
                w.WriteOctetString(digest);
            }
        }

        /// <summary>
        /// SpcIndirectDataContent: the data type and its encoded value,
        /// then the DigestInfo of what is signed.
        /// </summary>
        internal static byte[] IndirectData(string typeOid, byte[] value, HashAlgorithmName hash, byte[] digest)
        {
            var w = new AsnWriter(AsnEncodingRules.DER);
            using (w.PushSequence())
            {
                using (w.PushSequence())
                {
                    w.WriteObjectIdentifier(typeOid);
                    w.WriteEncodedValue(value);
                }
                WriteDigestInfo(w, hash, digest);
            }
            return w.Encode();
        }

        /// <summary>The data type, digest algorithm and digest of SpcIndirectDataContent.</summary>
        internal static (string Type, HashAlgorithmName Hash, byte[] Digest) ReadIndirectData(ReadOnlyMemory<byte> der)
        {
            var outer = new AsnReader(der, AsnEncodingRules.BER).ReadSequence();
            string type = outer.ReadSequence().ReadObjectIdentifier();
            var digestInfo = outer.ReadSequence();
            string algorithm = digestInfo.ReadSequence().ReadObjectIdentifier();
            byte[] digest = digestInfo.ReadOctetString();
            return (type, HashFromOid(algorithm), digest);
        }

        /// <summary>SpcSipInfo naming the PowerShell SIP, as a script's signature contains it.</summary>
        internal static byte[] PowerShellSipInfo()
        {
            var w = new AsnWriter(AsnEncodingRules.DER);
            using (w.PushSequence())
            {
                w.WriteInteger(65536);
                w.WriteOctetString(PowerShellSip.ToByteArray());
                for (int i = 0; i < 5; i++) w.WriteInteger(0);
            }
            return w.Encode();
        }

        /// <summary>SpcPeImageData as Windows' signing tools write it.</summary>
        internal static byte[] PeImageData()
        {
            var w = new AsnWriter(AsnEncodingRules.DER);
            using (w.PushSequence())
            {
                w.WriteBitString(ReadOnlySpan<byte>.Empty);
                using (w.PushSequence(new Asn1Tag(TagClass.ContextSpecific, 0, true)))
                using (w.PushSequence(new Asn1Tag(TagClass.ContextSpecific, 2, true)))
                {
                    w.WriteCharacterString(UniversalTagNumber.BMPString, "<<<Obsolete>>>", new Asn1Tag(TagClass.ContextSpecific, 0));
                }
            }
            return w.Encode();
        }

        static AsnEncodedData Attribute(string oid, Action<AsnWriter> write)
        {
            var w = new AsnWriter(AsnEncodingRules.DER);
            write(w);
            return new AsnEncodedData(oid, w.Encode());
        }

        /// <summary>
        /// A CMS signature by <paramref name="key"/> over
        /// <paramref name="content"/> of <paramref name="contentType"/>,
        /// with the Authenticode signed attributes when
        /// <paramref name="authenticode"/>, and an RFC 3161 timestamp from
        /// <paramref name="timestampUrl"/> when it is given.
        /// </summary>
        internal static byte[] Sign(string contentType, byte[] content, bool authenticode, Key key, HashAlgorithmName hash, string? timestampUrl)
        {
            var cms = new SignedCms(new ContentInfo(new Oid(contentType), content), false);
            var signer = new CmsSigner(SubjectIdentifierType.IssuerAndSerialNumber, key.Certificate, key.PrivateKey)
            {
                DigestAlgorithm = new Oid(HashOid(hash)),
                IncludeOption = X509IncludeOption.EndCertOnly,
            };
            signer.Certificates.AddRange(key.Chain);
            if (authenticode)
            {
                signer.SignedAttributes.Add(Attribute(OpusInfoOid, w => { using (w.PushSequence()) { } }));
                signer.SignedAttributes.Add(Attribute(StatementTypeOid, w => { using (w.PushSequence()) { w.WriteObjectIdentifier(IndividualPurposeOid); } }));
            }
            // Not silent, so a key in the Windows store that needs its PIN
            // can show the system's prompt for it.
            cms.ComputeSignature(signer, false);
            if (!string.IsNullOrEmpty(timestampUrl))
            {
                Timestamp(cms.SignerInfos[0], hash, timestampUrl!);
            }
            return cms.Encode();
        }

        /// <summary>
        /// Requests an RFC 3161 timestamp from <paramref name="url"/> over
        /// the signer's signature and adds the token as the unsigned attribute
        /// Windows reads, 1.3.6.1.4.1.311.3.3.1.
        /// </summary>
        static void Timestamp(SignerInfo signerInfo, HashAlgorithmName hash, string url)
        {
            var request = Rfc3161TimestampRequest.CreateFromSignerInfo(signerInfo, hash, null, RandomNumberGenerator.GetBytes(8), true, null);
            using var body = new ByteArrayContent(request.Encode());
            body.Headers.ContentType = new MediaTypeHeaderValue("application/timestamp-query");
            HttpResponseMessage response;
            try
            {
                response = Http.PostAsync(url, body).GetAwaiter().GetResult();
            }
            catch (HttpRequestException e)
            {
                throw new SigningException($"the timestamp server {url} could not be reached: {e.Message}");
            }
            using (response)
            {
                if (!response.IsSuccessStatusCode)
                {
                    throw new SigningException($"the timestamp server {url} replied {(int)response.StatusCode} {response.ReasonPhrase}");
                }
                byte[] reply = response.Content.ReadAsByteArrayAsync().GetAwaiter().GetResult();
                Rfc3161TimestampToken token;
                try
                {
                    token = request.ProcessResponse(reply, out _);
                }
                catch (CryptographicException e)
                {
                    throw new SigningException($"the timestamp server {url} sent a reply that is not a timestamp for this signature: {e.Message}");
                }
                signerInfo.AddUnsignedAttribute(new AsnEncodedData(Rfc3161CounterSignOid, token.AsSignedCms().Encode()));
            }
        }

        /// <summary>
        /// The CMS signature in <paramref name="der"/> and why it is not
        /// valid, null when its first signer's signature checks against the
        /// certificate it names. Whether that certificate is trusted is not
        /// checked.
        /// </summary>
        internal static (SignedCms? Cms, string? Fault) Check(byte[] der)
        {
            var cms = new SignedCms();
            try
            {
                cms.Decode(der);
            }
            catch (CryptographicException e)
            {
                return (null, $"its signature cannot be read: {e.Message}");
            }
            if (cms.SignerInfos.Count == 0)
            {
                return (null, "its signature has no signer");
            }
            try
            {
                cms.SignerInfos[0].CheckSignature(true);
            }
            catch (CryptographicException e)
            {
                return (null, $"its signature does not check: {e.Message}");
            }
            return (cms, null);
        }

        /// <summary>Who signed: the first signer's certificate's subject.</summary>
        internal static string Signer(SignedCms cms) => cms.SignerInfos[0].Certificate?.Subject ?? "an unnamed signer";
    }

    /// <summary>A refusal that names the file and what to do, as one line.</summary>
    public sealed class SigningException : Exception
    {
        public SigningException(string message) : base(message) { }
    }
}
