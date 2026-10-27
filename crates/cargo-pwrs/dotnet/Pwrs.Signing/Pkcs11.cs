using System;
using System.Collections.Generic;
using System.Formats.Asn1;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;

namespace Pwrs.Signing
{
    /// <summary>
    /// A private key on a PKCS#11 token, found by the token's label and
    /// its own, and signed with through the token's library while this
    /// lives: one session, logged in with the PIN.
    /// </summary>
    internal sealed unsafe class Pkcs11Token : IDisposable
    {
        const nuint CKR_OK = 0;
        const nuint CKR_USER_ALREADY_LOGGED_IN = 0x100;
        const nuint CKR_CRYPTOKI_ALREADY_INITIALIZED = 0x191;
        const nuint CKF_SERIAL_SESSION = 0x4;
        const nuint CKU_USER = 1;
        const nuint CKO_PRIVATE_KEY = 3;
        const nuint CKA_CLASS = 0;
        const nuint CKA_LABEL = 3;
        const nuint CKM_RSA_PKCS = 1;
        const nuint CKM_ECDSA = 0x1041;

        // The functions' places in CK_FUNCTION_LIST, after its version.
        const int C_Initialize = 0, C_Finalize = 1, C_GetSlotList = 4, C_GetTokenInfo = 6, C_OpenSession = 12, C_CloseSession = 13,
            C_Login = 18, C_Logout = 19, C_FindObjectsInit = 26, C_FindObjects = 27, C_FindObjectsFinal = 28, C_SignInit = 42, C_Sign = 43;

        // The PKCS#11 headers pack every structure to one byte on Windows,
        // where a CK_ULONG is 32 bits. Elsewhere structures take their
        // natural alignment and a CK_ULONG is as wide as a pointer, so on
        // every target a CK_ULONG travels in a nuint whose high bytes are
        // zero.
        static readonly bool Windows = OperatingSystem.IsWindows();
        static readonly int UlongSize = Windows ? 4 : IntPtr.Size;

        readonly IntPtr _library;
        readonly IntPtr _functions;
        readonly string _what;
        readonly bool _ec;
        bool _finalize;
        nuint _session;
        bool _loggedIn;
        nuint _key;

        internal Pkcs11Token(string library, string token, string key, string pin, bool ec)
        {
            _what = $"the PKCS#11 library {library}";
            _ec = ec;
            try
            {
                _library = NativeLibrary.Load(library);
            }
            catch (Exception e) when (e is DllNotFoundException || e is BadImageFormatException)
            {
                throw new SigningException($"{_what} cannot be loaded: {e.Message}");
            }
            try
            {
                if (!NativeLibrary.TryGetExport(_library, "C_GetFunctionList", out IntPtr getList))
                {
                    throw new SigningException($"{_what} exports no C_GetFunctionList, so it is not a PKCS#11 library");
                }
                IntPtr list = IntPtr.Zero;
                Check(((delegate* unmanaged[Cdecl]<IntPtr*, nuint>)getList)(&list), "C_GetFunctionList");
                _functions = list;
                nuint rv = Mask(((delegate* unmanaged[Cdecl]<IntPtr, nuint>)Fn(C_Initialize))(IntPtr.Zero));
                if (rv != CKR_OK && rv != CKR_CRYPTOKI_ALREADY_INITIALIZED) Fail(rv, "C_Initialize");
                _finalize = rv == CKR_OK;
                nuint slot = FindSlot(token);
                nuint session = 0;
                Check(((delegate* unmanaged[Cdecl]<nuint, nuint, IntPtr, IntPtr, nuint*, nuint>)Fn(C_OpenSession))(slot, CKF_SERIAL_SESSION, IntPtr.Zero, IntPtr.Zero, &session), "C_OpenSession");
                _session = session;
                byte[] pinBytes = Encoding.UTF8.GetBytes(pin);
                fixed (byte* p = pinBytes)
                {
                    rv = Mask(((delegate* unmanaged[Cdecl]<nuint, nuint, byte*, nuint, nuint>)Fn(C_Login))(_session, CKU_USER, p, (nuint)pinBytes.Length));
                }
                if (rv != CKR_OK && rv != CKR_USER_ALREADY_LOGGED_IN) Fail(rv, "C_Login");
                _loggedIn = rv == CKR_OK;
                _key = FindKey(token, key);
            }
            catch
            {
                Dispose();
                throw;
            }
        }

        /// <summary>
        /// The token's signature over <paramref name="digest"/>: PKCS#1
        /// v1.5 over its DigestInfo for an RSA key, r and s side by side for
        /// an ECDSA one.
        /// </summary>
        internal byte[] Sign(byte[] digest, HashAlgorithmName hash)
        {
            byte[] data = digest;
            if (!_ec)
            {
                var w = new AsnWriter(AsnEncodingRules.DER);
                Cms.WriteDigestInfo(w, hash, digest);
                data = w.Encode();
            }
            var mechanism = new byte[UlongSize + IntPtr.Size + UlongSize];
            fixed (byte* m = mechanism)
            {
                WriteUlong(m, _ec ? CKM_ECDSA : CKM_RSA_PKCS);
                Check(((delegate* unmanaged[Cdecl]<nuint, byte*, nuint, nuint>)Fn(C_SignInit))(_session, m, _key), "C_SignInit");
            }
            var sign = (delegate* unmanaged[Cdecl]<nuint, byte*, nuint, byte*, nuint*, nuint>)Fn(C_Sign);
            nuint length = 0;
            fixed (byte* d = data)
            {
                Check(sign(_session, d, (nuint)data.Length, null, &length), "C_Sign");
                var signature = new byte[(int)length];
                fixed (byte* s = signature)
                {
                    Check(sign(_session, d, (nuint)data.Length, s, &length), "C_Sign");
                }
                return signature.AsSpan(0, (int)length).ToArray();
            }
        }

        /// <summary>
        /// Logs out, closes the session and finalizes the library. The
        /// library stays loaded until the process exits, so cleanup that
        /// the module or the libraries it uses run at exit finds its code
        /// still there.
        /// </summary>
        public void Dispose()
        {
            if (_functions != IntPtr.Zero)
            {
                if (_loggedIn) ((delegate* unmanaged[Cdecl]<nuint, nuint>)Fn(C_Logout))(_session);
                if (_session != 0) ((delegate* unmanaged[Cdecl]<nuint, nuint>)Fn(C_CloseSession))(_session);
                if (_finalize) ((delegate* unmanaged[Cdecl]<IntPtr, nuint>)Fn(C_Finalize))(IntPtr.Zero);
            }
            _loggedIn = false;
            _session = 0;
            _finalize = false;
        }

        nuint FindSlot(string token)
        {
            var list = (delegate* unmanaged[Cdecl]<byte, byte*, nuint*, nuint>)Fn(C_GetSlotList);
            nuint count = 0;
            Check(list(1, null, &count), "C_GetSlotList");
            var slots = new byte[((int)count + 1) * UlongSize];
            fixed (byte* s = slots)
            {
                Check(list(1, s, &count), "C_GetSlotList");
            }
            var labels = new List<string>();
            var info = new byte[1024];
            for (int i = 0; i < (int)count; i++)
            {
                nuint slot;
                fixed (byte* s = slots) slot = ReadUlong(s + i * UlongSize);
                Array.Clear(info);
                fixed (byte* p = info)
                {
                    Check(((delegate* unmanaged[Cdecl]<nuint, byte*, nuint>)Fn(C_GetTokenInfo))(slot, p), "C_GetTokenInfo");
                }
                string label = Encoding.UTF8.GetString(info, 0, 32).TrimEnd(' ', '\0');
                if (label == token) return slot;
                labels.Add(label);
            }
            throw new SigningException($"{_what} has no token labelled {token}; its tokens are {(labels.Count == 0 ? "none" : string.Join(", ", labels))}");
        }

        nuint FindKey(string token, string label)
        {
            int attribute = UlongSize + IntPtr.Size + UlongSize;
            var template = new byte[2 * attribute];
            var keyClass = new byte[UlongSize];
            byte[] labelBytes = Encoding.UTF8.GetBytes(label);
            var found = new byte[2 * UlongSize];
            nuint count = 0;
            nuint rv;
            fixed (byte* t = template, c = keyClass, l = labelBytes, f = found)
            {
                WriteUlong(c, CKO_PRIVATE_KEY);
                WriteAttribute(t, CKA_CLASS, c, UlongSize);
                WriteAttribute(t + attribute, CKA_LABEL, l, labelBytes.Length);
                Check(((delegate* unmanaged[Cdecl]<nuint, byte*, nuint, nuint>)Fn(C_FindObjectsInit))(_session, t, 2), "C_FindObjectsInit");
                rv = Mask(((delegate* unmanaged[Cdecl]<nuint, byte*, nuint, nuint*, nuint>)Fn(C_FindObjects))(_session, f, 2, &count));
                ((delegate* unmanaged[Cdecl]<nuint, nuint>)Fn(C_FindObjectsFinal))(_session);
                if (rv != CKR_OK) Fail(rv, "C_FindObjects");
                if (count == 0) throw new SigningException($"the token {token} has no private key labelled {label}");
                if (count > 1) throw new SigningException($"the token {token} has more than one private key labelled {label}");
                return ReadUlong(f);
            }
        }

        IntPtr Fn(int index) => Marshal.ReadIntPtr(_functions, (Windows ? 2 : IntPtr.Size) + index * IntPtr.Size);

        static nuint Mask(nuint rv) => Windows ? (nuint)(uint)rv : rv;

        static void WriteUlong(byte* at, nuint value)
        {
            if (UlongSize == 4) Unsafe.WriteUnaligned(at, (uint)value);
            else Unsafe.WriteUnaligned(at, (ulong)value);
        }

        static nuint ReadUlong(byte* at) => UlongSize == 4 ? Unsafe.ReadUnaligned<uint>(at) : (nuint)Unsafe.ReadUnaligned<ulong>(at);

        static void WriteAttribute(byte* at, nuint type, byte* value, int length)
        {
            WriteUlong(at, type);
            Unsafe.WriteUnaligned(at + UlongSize, (IntPtr)value);
            WriteUlong(at + UlongSize + IntPtr.Size, (nuint)length);
        }

        void Check(nuint rv, string function)
        {
            rv = Mask(rv);
            if (rv != CKR_OK) Fail(rv, function);
        }

        void Fail(nuint rv, string function)
        {
            string why = (ulong)rv switch
            {
                0x05 => "the token reported a general error",
                0x30 => "the device reported an error",
                0x63 => "the key is not of the kind the certificate's key is",
                0x68 => "the key cannot sign",
                0x70 => "the token cannot sign with that mechanism",
                0xA0 => "the PIN is wrong",
                0xA4 => "the PIN is locked",
                0xE0 => "the token is not present",
                0x101 => "the token was not logged in",
                _ => "the token refused",
            };
            throw new SigningException($"{function} in {_what} returned 0x{(ulong)rv:X}: {why}");
        }
    }
}
