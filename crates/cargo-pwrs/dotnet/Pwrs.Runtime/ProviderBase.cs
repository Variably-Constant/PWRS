using System;
using System.Collections;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Management.Automation;
using System.Management.Automation.Provider;
using System.Runtime.InteropServices;
using System.Threading;

namespace Pwrs
{
    /// <summary>
    /// A drive served by one Rust provider instance. The pointer is
    /// taken exactly once: by RemoveDrive after the native side has
    /// freed the instance, or by the finalizer, which frees it, for a
    /// drive that was never removed.
    /// </summary>
    public sealed class PwrsDriveInfo : PSDriveInfo
    {
        private readonly NativeModule _module;
        private readonly uint _providerId;
        private IntPtr _instance;

        /// <summary>Serializes the operations on this drive; a Rust operation may mutate the instance.</summary>
        internal readonly object Gate = new object();

        internal PwrsDriveInfo(string name, ProviderInfo provider, string root, string description, PSCredential? credential, NativeModule module, uint providerId, IntPtr instance)
            : base(name, provider, root, description, credential)
        {
            _module = module;
            _providerId = providerId;
            _instance = instance;
        }

        internal IntPtr Instance => _instance;

        internal IntPtr Take() => Interlocked.Exchange(ref _instance, IntPtr.Zero);

        ~PwrsDriveInfo()
        {
            IntPtr instance = Take();
            if (instance != IntPtr.Zero) ProviderBase.DropInstance(_module, _providerId, instance);
        }
    }

    /// <summary>
    /// Base of every generated provider. The generated subclass gives
    /// the module and the provider id; this class implements the
    /// NavigationCmdletProvider surface by forwarding each operation to
    /// the native provider, against the instance behind the current
    /// drive, and writing the returned rows to the engine. An item row
    /// is [path, value, isContainer]; a drive row is
    /// [name, root, instance].
    /// </summary>
    public abstract class ProviderBase : NavigationCmdletProvider, IContentCmdletProvider
    {
        protected abstract NativeModule Module { get; }
        protected abstract uint ProviderId { get; }

        // op codes, mirrored from pwrs::provider::op.
        private const uint IsValidPathOp = 0, ItemExistsOp = 1, IsItemContainerOp = 2, GetItemOp = 3, SetItemOp = 4, ClearItemOp = 5;
        private const uint GetChildItemsOp = 6, GetChildNamesOp = 7, HasChildItemsOp = 8, NewItemOp = 9, RemoveItemOp = 10;
        private const uint RenameItemOp = 11, CopyItemOp = 12, GetContentOp = 13, SetContentOp = 14, ClearContentOp = 15;
        private const uint NewDriveOp = 16, RemoveDriveOp = 17, InitDefaultDrivesOp = 18;
        private const uint MakePathOp = 19, GetParentPathOp = 20, GetChildNameOp = 21, NormalizeRelativePathOp = 22;
        private const uint DropDriveOp = 23;

        /// <summary>The drive the engine set for this operation, when it is one of ours.</summary>
        private PwrsDriveInfo? Drive => PSDriveInfo as PwrsDriveInfo;

        /// <summary>
        /// One operation against the current drive's instance, or with
        /// no instance when the engine has set no drive of ours.
        /// </summary>
        private object?[] Call(uint op, params object?[] args)
        {
            PwrsDriveInfo? drive = Drive;
            if (drive == null) return Invoke(Module, ProviderId, op, IntPtr.Zero, args);
            lock (drive.Gate)
            {
                return Invoke(Module, ProviderId, op, drive.Instance, args);
            }
        }

        private static unsafe object?[] Invoke(NativeModule module, uint providerId, uint op, IntPtr instance, object?[] args)
        {
            IntPtr argsHandle = GCHandle.ToIntPtr(GCHandle.Alloc(args));
            IntPtr result = IntPtr.Zero;
            IntPtr err = IntPtr.Zero;
            int status = module.ProviderInvoke(providerId, op, instance, argsHandle, &result, &err);
            GCHandle.FromIntPtr(argsHandle).Free();
            object? errObj = Native.TakeErr(err);
            if (status != Native.Ok)
            {
                throw new PwrsException(errObj as string ?? $"provider op {op} failed with status {status}");
            }
            object? rows = Native.TakeTarget(result);
            if (rows is object?[] arr) return arr;
            return Array.Empty<object?>();
        }

        /// <summary>
        /// Frees a drive's instance without running remove_drive; the
        /// finalizer of a drive that was never removed calls this, so a
        /// failure is reported on stderr rather than thrown.
        /// </summary>
        internal static void DropInstance(NativeModule module, uint providerId, IntPtr instance)
        {
            try
            {
                Invoke(module, providerId, DropDriveOp, instance, Array.Empty<object?>());
            }
            catch (Exception e)
            {
                Console.Error.WriteLine($"pwrs: freeing a provider drive instance failed: {e.Message}");
            }
        }

        private static object?[] Row(object? r) => r is PSObject ps ? (ps.BaseObject as object?[] ?? Array.Empty<object?>()) : (r as object?[] ?? Array.Empty<object?>());

        private static string Str(object? o) => (o is PSObject ps ? ps.BaseObject : o)?.ToString() ?? string.Empty;
        private static bool Bool(object? o) => LanguagePrimitives.ConvertTo<bool>(o is PSObject ps ? ps.BaseObject : o);
        private static IntPtr Ptr(object? o) => (IntPtr)LanguagePrimitives.ConvertTo<long>(o is PSObject ps ? ps.BaseObject : o);

        private void WriteRow(object? row)
        {
            object?[] cells = Row(row);
            if (cells.Length < 3) throw new PwrsException($"an item row needs [path, value, isContainer]; got {cells.Length} cells");
            WriteItemObject(cells[1], Str(cells[0]), Bool(cells[2]));
        }

        private PwrsDriveInfo DriveFromRow(object? row, string description, PSCredential? credential)
        {
            object?[] c = Row(row);
            if (c.Length < 3) throw new PwrsException($"a drive row needs [name, root, instance]; got {c.Length} cells");
            return new PwrsDriveInfo(Str(c[0]), ProviderInfo, Str(c[1]), description, credential, Module, ProviderId, Ptr(c[2]));
        }

        // ---- drives ----
        protected override Collection<PSDriveInfo> InitializeDefaultDrives()
        {
            var drives = new Collection<PSDriveInfo>();
            foreach (object? r in Call(InitDefaultDrivesOp)) drives.Add(DriveFromRow(r, string.Empty, null));
            return drives;
        }

        protected override PSDriveInfo NewDrive(PSDriveInfo drive)
        {
            object?[] rows = Call(NewDriveOp, drive.Name, drive.Root);
            if (rows.Length == 0) throw new PwrsException("new_drive returned no drive row");
            return DriveFromRow(rows[0], drive.Description, drive.Credential);
        }

        /// <summary>
        /// Runs remove_drive on the instance and frees it; an error
        /// keeps the instance with the drive and propagates.
        /// </summary>
        protected override PSDriveInfo RemoveDrive(PSDriveInfo drive)
        {
            if (drive is PwrsDriveInfo d)
            {
                lock (d.Gate)
                {
                    IntPtr instance = d.Instance;
                    if (instance != IntPtr.Zero)
                    {
                        Invoke(Module, ProviderId, RemoveDriveOp, instance, Array.Empty<object?>());
                        d.Take();
                    }
                }
            }
            return drive;
        }

        // ---- items ----
        protected override bool IsValidPath(string path) => Bool(First(Call(IsValidPathOp, path), true));
        protected override bool ItemExists(string path) => Bool(First(Call(ItemExistsOp, path), false));
        protected override bool IsItemContainer(string path) => Bool(First(Call(IsItemContainerOp, path), false));

        protected override void GetItem(string path)
        {
            foreach (object? r in Call(GetItemOp, path)) WriteRow(r);
        }

        protected override void SetItem(string path, object value)
        {
            foreach (object? r in Call(SetItemOp, path, value)) WriteRow(r);
        }

        protected override void ClearItem(string path) => Call(ClearItemOp, path);

        // ---- containers ----
        protected override void GetChildItems(string path, bool recurse)
        {
            foreach (object? r in Call(GetChildItemsOp, path, recurse)) WriteRow(r);
        }

        protected override void GetChildNames(string path, ReturnContainers returnContainers)
        {
            foreach (object? r in Call(GetChildNamesOp, path)) WriteItemObject(Str(r), Str(r), false);
        }

        protected override bool HasChildItems(string path) => Bool(First(Call(HasChildItemsOp, path), false));

        protected override void NewItem(string path, string itemTypeName, object newItemValue)
        {
            foreach (object? r in Call(NewItemOp, path, itemTypeName ?? string.Empty, newItemValue)) WriteRow(r);
        }

        protected override void RemoveItem(string path, bool recurse)
        {
            if (ShouldProcess(path, "Remove")) Call(RemoveItemOp, path, recurse);
        }

        protected override void RenameItem(string path, string newName)
        {
            if (ShouldProcess(path, "Rename")) foreach (object? r in Call(RenameItemOp, path, newName)) WriteRow(r);
        }

        protected override void CopyItem(string path, string copyPath, bool recurse)
        {
            foreach (object? r in Call(CopyItemOp, path, copyPath, recurse)) WriteRow(r);
        }

        // ---- navigation ----
        protected override string GetChildName(string path) => Str(First(Call(GetChildNameOp, path), path));
        protected override string GetParentPath(string path, string root) => Str(First(Call(GetParentPathOp, path, root ?? string.Empty), string.Empty));
        protected override string MakePath(string parent, string child) => Str(First(Call(MakePathOp, parent ?? string.Empty, child ?? string.Empty), child));

        // ---- content ----
        /// <summary>
        /// Each result as the module returned it, the PSObject a wrapped
        /// one comes in included, so the properties that live on the
        /// wrapper, a deserialized object's among them, reach the reader.
        /// </summary>
        public IContentReader GetContentReader(string path)
        {
            var lines = new List<object?>();
            foreach (object? r in Call(GetContentOp, path)) lines.Add(r);
            return new ListReader(lines);
        }

        public IContentWriter GetContentWriter(string path) => new ListWriter(this, path);

        public void ClearContent(string path) => Call(ClearContentOp, path);

        public object GetContentReaderDynamicParameters(string path) => null!;
        public object GetContentWriterDynamicParameters(string path) => null!;
        public object ClearContentDynamicParameters(string path) => null!;

        internal void SetContent(string path, object?[] content)
        {
            var args = new object?[content.Length + 1];
            args[0] = path;
            Array.Copy(content, 0, args, 1, content.Length);
            Call(SetContentOp, args);
        }

        private static object? First(object?[] rows, object? fallback) => rows.Length > 0 ? (rows[0] is PSObject ps ? ps.BaseObject : rows[0]) : fallback;

        private sealed class ListReader : IContentReader
        {
            private readonly List<object?> _items;
            private int _i;
            public ListReader(List<object?> items) { _items = items; }
            public IList Read(long readCount)
            {
                var outList = new List<object?>();
                long n = readCount <= 0 ? long.MaxValue : readCount;
                while (_i < _items.Count && outList.Count < n) outList.Add(_items[_i++]);
                return outList;
            }
            public void Seek(long offset, System.IO.SeekOrigin origin) => throw new NotSupportedException();
            public void Close() { }
            public void Dispose() { }
        }

        private sealed class ListWriter : IContentWriter
        {
            private readonly ProviderBase _p;
            private readonly string _path;
            private readonly List<object?> _buffer = new List<object?>();
            public ListWriter(ProviderBase p, string path) { _p = p; _path = path; }
            public IList Write(IList content)
            {
                foreach (object? o in content) _buffer.Add(o);
                return content;
            }
            public void Seek(long offset, System.IO.SeekOrigin origin) => throw new NotSupportedException();
            public void Close() => _p.SetContent(_path, _buffer.ToArray());
            public void Dispose() { }
        }
    }
}
