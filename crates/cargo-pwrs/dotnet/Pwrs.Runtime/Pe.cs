using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;

namespace Pwrs
{
    /// <summary>
    /// What a PE file's headers say: whether it is one, its machine, the
    /// DLLs its import directory names, in order, and the symbol file its
    /// debug directory names. Delay-loaded imports are not among the
    /// imports, as a load does not need them.
    /// </summary>
    internal sealed class Pe
    {
        internal bool IsPe;
        internal ushort Machine;
        internal readonly List<string> Imports = new List<string>();

        /// <summary>The headers every read starts from.</summary>
        private sealed class Layout
        {
            internal ushort Machine;
            internal byte[] Optional = Array.Empty<byte>();
            /// <summary>Where the data directories start in <see cref="Optional"/>.</summary>
            internal int Directories;
            internal byte[] Sections = Array.Empty<byte>();
            internal uint Headers;
        }

        internal static Pe Read(string path)
        {
            var pe = new Pe();
            using (FileStream file = Open(path))
            {
                Layout? layout = ReadLayout(file, path);
                if (layout == null) return pe;
                pe.IsPe = true;
                pe.Machine = layout.Machine;
                uint imports = DataDirectory(layout, 1, path);
                if (imports == 0) return pe;
                for (long at = Offset(imports, layout, path); ; at += 20)
                {
                    byte[] descriptor = At(file, at, 20, path);
                    uint name = U32(descriptor, 12, path);
                    if (name == 0) break;
                    pe.Imports.Add(Text(file, Offset(name, layout, path), path));
                }
            }
            return pe;
        }

        /// <summary>
        /// The file name of the .pdb the image at <paramref name="path"/>
        /// records in its CodeView debug entry, the name a debugger searches
        /// for beside the image; null for a file that is not a PE file or
        /// records none. Of a recorded path, only the last part.
        /// </summary>
        internal static string? RecordedPdb(string path)
        {
            using (FileStream file = Open(path))
            {
                Layout? layout = ReadLayout(file, path);
                if (layout == null) return null;
                uint debug = DataDirectory(layout, 6, path);
                if (debug == 0) return null;
                uint size = U32(layout.Optional, layout.Directories + 6 * 8 + 4, path);
                long table = Offset(debug, layout, path);
                for (long at = table; at + 28 <= table + size; at += 28)
                {
                    byte[] entry = At(file, at, 28, path);
                    // Type 2 is CodeView. Its data is "RSDS", a GUID, an
                    // age, and the path, and the entry gives where it is in
                    // the file.
                    if (U32(entry, 12, path) != 2) continue;
                    long data = U32(entry, 24, path);
                    if (U32(At(file, data, 4, path), 0, path) != 0x53445352) continue;
                    string recorded = Text(file, data + 24, path);
                    string name = recorded.Substring(recorded.LastIndexOfAny(new[] { '\\', '/' }) + 1);
                    return name.Length == 0 ? null : name;
                }
            }
            return null;
        }

        private static FileStream Open(string path) => new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);

        /// <summary>The headers of <paramref name="file"/>, or null for a file that is not a PE file.</summary>
        private static Layout? ReadLayout(FileStream file, string path)
        {
            if (file.Length < 64) return null;
            byte[] dos = At(file, 0, 64, path);
            if (U16(dos, 0, path) != 0x5a4d) return null;
            long nt = U32(dos, 0x3c, path);
            if (nt + 24 > file.Length) return null;
            byte[] head = At(file, nt, 24, path);
            if (U32(head, 0, path) != 0x00004550) return null;
            int optionalSize = U16(head, 20, path);
            var layout = new Layout { Machine = U16(head, 4, path), Optional = At(file, nt + 24, optionalSize, path) };
            layout.Directories = U16(layout.Optional, 0, path) switch
            {
                0x10b => 96,
                0x20b => 112,
                _ => throw new InvalidDataException(path + " has an optional header of no kind PE defines"),
            };
            layout.Headers = U32(layout.Optional, 60, path);
            layout.Sections = At(file, nt + 24 + optionalSize, 40 * U16(head, 6, path), path);
            return layout;
        }

        /// <summary>The address data directory <paramref name="index"/> gives, or 0 when the image has none.</summary>
        private static uint DataDirectory(Layout layout, int index, string path)
            => U32(layout.Optional, layout.Directories - 4, path) > index ? U32(layout.Optional, layout.Directories + index * 8, path) : 0;

        /// <summary>The file offset of an address the image maps.</summary>
        private static long Offset(uint rva, Layout layout, string path)
        {
            byte[] sections = layout.Sections;
            for (int s = 0; s + 40 <= sections.Length; s += 40)
            {
                uint virtualSize = U32(sections, s + 8, path);
                uint address = U32(sections, s + 12, path);
                uint rawSize = U32(sections, s + 16, path);
                uint raw = U32(sections, s + 20, path);
                if (rva >= address && rva - address < Math.Max(virtualSize, rawSize)) return (long)raw + (rva - address);
            }
            if (rva < layout.Headers) return rva;
            throw new InvalidDataException(path + " names an address 0x" + rva.ToString("x", CultureInfo.InvariantCulture) + " that is in no section");
        }

        private static byte[] At(FileStream file, long at, int count, string path)
        {
            if (at < 0 || count < 0 || at + count > file.Length) throw Truncated(path);
            var bytes = new byte[count];
            file.Position = at;
            for (int read = 0; read < count;)
            {
                int n = file.Read(bytes, read, count - read);
                if (n <= 0) throw Truncated(path);
                read += n;
            }
            return bytes;
        }

        /// <summary>The UTF-8 text at <paramref name="at"/>, up to the NUL that ends it.</summary>
        private static string Text(FileStream file, long at, string path)
        {
            if (at < 0 || at >= file.Length) throw Truncated(path);
            file.Position = at;
            var bytes = new List<byte>();
            for (int c = file.ReadByte(); c != 0; c = file.ReadByte())
            {
                if (c < 0) throw Truncated(path);
                bytes.Add((byte)c);
            }
            return Encoding.UTF8.GetString(bytes.ToArray());
        }

        private static ushort U16(byte[] b, int at, string path)
            => at >= 0 && at + 2 <= b.Length ? BitConverter.ToUInt16(b, at) : throw Truncated(path);

        private static uint U32(byte[] b, int at, string path)
            => at >= 0 && at + 4 <= b.Length ? BitConverter.ToUInt32(b, at) : throw Truncated(path);

        private static InvalidDataException Truncated(string path) => new InvalidDataException(path + " ends inside its PE headers");
    }
}
