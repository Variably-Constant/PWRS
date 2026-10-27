using System;
using System.Collections;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Management.Automation;
using System.Management.Automation.Provider;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Threading;

namespace Pwrs
{
    /// <summary>
    /// A drive backed by one Rust provider instance. The pointer is
    /// taken exactly once: by RemoveDrive after the native side has
    /// freed the instance, or by the finalizer, which frees it, for a
    /// drive that was never removed. Its root is its prefix followed by
    /// the root the native provider gave, so every provider path on the
    /// drive, a piped item's PSPath among them, names the drive.
    /// </summary>
    public sealed class PwrsDriveInfo : PSDriveInfo
    {
        private readonly NativeModule _module;
        private readonly uint _providerId;
        private IntPtr _instance;

        /// <summary>
        /// Entered for each operation on this drive, so operations from
        /// different threads run one at a time, because a Rust operation
        /// may mutate the instance. Every entry is shared, so an operation
        /// reached again on the thread running one nests inside it.
        /// </summary>
        internal readonly Gate Gate = new Gate();

        /// <summary>
        /// The start of every path on this drive: its name, a colon and
        /// the separator PowerShell writes paths with on this system, a
        /// backslash on Windows and a slash elsewhere. PowerShell takes a
        /// path as the drive's root only when it ends in that separator;
        /// with the other one, commands on the root fail.
        /// </summary>
        internal readonly string Prefix;

        /// <summary>
        /// The drive's place among the drives of this class made in the
        /// process. An operation between two drives enters their gates in
        /// this order, so two such operations going opposite ways never
        /// wait for each other.
        /// </summary>
        internal readonly long Order;

        private static long s_made;

        internal PwrsDriveInfo(string name, ProviderInfo provider, string nativeRoot, string description, PSCredential? credential, NativeModule module, uint providerId, IntPtr instance)
            : base(name, provider, PrefixOf(name) + nativeRoot, description, credential)
        {
            _module = module;
            _providerId = providerId;
            _instance = instance;
            Prefix = PrefixOf(name);
            Order = Interlocked.Increment(ref s_made);
        }

        private static string PrefixOf(string name) => name + ":" + System.IO.Path.DirectorySeparatorChar;

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
    /// NavigationCmdletProvider surface, content, item properties and
    /// security descriptors by forwarding each operation to the native
    /// provider, against the instance behind the drive a path is on, and
    /// writing the returned rows to the engine. An item row is
    /// [path, value, isContainer]; a drive row is [name, root, instance].
    /// The native provider takes and returns paths without the drive's
    /// prefix, which this class strips and puts back.
    /// </summary>
    public abstract class ProviderBase : NavigationCmdletProvider, IContentCmdletProvider, IPropertyCmdletProvider, IDynamicPropertyCmdletProvider, ISecurityDescriptorCmdletProvider
    {
        protected abstract NativeModule Module { get; }
        protected abstract uint ProviderId { get; }

        // op codes, mirrored from `pwrs::provider::op`.
        private const uint IsValidPathOp = 0, ItemExistsOp = 1, IsItemContainerOp = 2, GetItemOp = 3, SetItemOp = 4, ClearItemOp = 5;
        private const uint GetChildItemsOp = 6, GetChildNamesOp = 7, HasChildItemsOp = 8, NewItemOp = 9, RemoveItemOp = 10;
        private const uint RenameItemOp = 11, CopyItemOp = 12, GetContentOp = 13, WriteContentOp = 14, ClearContentOp = 15;
        private const uint NewDriveOp = 16, RemoveDriveOp = 17, InitDefaultDrivesOp = 18;
        private const uint MakePathOp = 19, GetParentPathOp = 20, GetChildNameOp = 21, NormalizeRelativePathOp = 22;
        private const uint DropDriveOp = 23;
        private const uint GetPropertyOp = 24, SetPropertyOp = 25, ClearPropertyOp = 26, NewPropertyOp = 27, RemovePropertyOp = 28;
        private const uint RenamePropertyOp = 29, CopyPropertyOp = 30, MovePropertyOp = 31;
        private const uint GetSecurityDescriptorOp = 32, SetSecurityDescriptorOp = 33;
        private const uint CopyItemToOp = 34, CopyPropertyToOp = 35, MovePropertyToOp = 36;
        private const uint MoveItemOp = 37, MoveItemToOp = 38, InvokeDefaultActionOp = 39, ExpandPathOp = 40;

        // Security descriptor kinds, mirrored from `pwrs::provider::SecurityKind`.
        private const int FileKind = 0, DirectoryKind = 1, RegistryKeyKind = 2, OtherKind = 3;

        // ---- routing ----

        /// <summary>
        /// The drive a path is on, and the path as the native provider
        /// takes it, without the drive's prefix. The drive the engine set
        /// for the operation is tried first. A path given with no drive,
        /// such as a piped item's PSPath, goes to the drive of this
        /// provider whose prefix starts it. A path that names no drive
        /// goes on as it is, with the drive the engine set, if any.
        /// </summary>
        private (PwrsDriveInfo? Drive, string Path) Route(string? path) => Locate(path, out _);

        /// <summary>
        /// <see cref="Route"/>, and whether the path began with a drive's
        /// prefix. PowerShell passes the path methods some paths with the
        /// prefix, and some with only the drive's name and colon taken off,
        /// which it adds back to the result itself. So the path methods
        /// add the prefix back only to a path that had it.
        /// </summary>
        private (PwrsDriveInfo? Drive, string Path) Locate(string? path, out bool qualified)
        {
            string p = path ?? string.Empty;
            qualified = true;
            PwrsDriveInfo? current = PSDriveInfo as PwrsDriveInfo;
            if (current != null && Strip(current, p, out string inner)) return (current, inner);
            foreach (PSDriveInfo d in ProviderInfo.Drives)
            {
                if (d is PwrsDriveInfo drive && Strip(drive, p, out string rest)) return (drive, rest);
            }
            qualified = false;
            return (current, p);
        }

        /// <summary>
        /// The rest of <paramref name="path"/> after the drive's name, its
        /// colon and one separator of either kind; the name and colon
        /// alone name the root. The name matches without regard to case.
        /// </summary>
        private static bool Strip(PwrsDriveInfo drive, string path, out string rest)
        {
            string name = drive.Name + ":";
            rest = path;
            if (path.Length < name.Length || string.Compare(path, 0, name, 0, name.Length, StringComparison.OrdinalIgnoreCase) != 0) return false;
            if (path.Length == name.Length)
            {
                rest = string.Empty;
                return true;
            }
            char after = path[name.Length];
            if (after != '\\' && after != '/') return false;
            rest = path.Substring(name.Length + 1);
            return true;
        }

        /// <summary>A path from the native provider, with the drive's prefix put back.</summary>
        private static string Full(PwrsDriveInfo? drive, string nativePath) => drive == null ? nativePath : drive.Prefix + nativePath;

        /// <summary>
        /// The path a path method passes to the native provider, and the
        /// text it adds back to the path the native provider returns. That
        /// text is the drive's prefix, for a path that had it, or this
        /// system's separator, for a path PowerShell passed with only the
        /// drive's name and colon taken off, as Split-Path does. Either way
        /// the native provider gets a path without them, as in every other
        /// operation, and the path returned starts as a FileSystem drive's
        /// does, whichever separator the path given had.
        /// </summary>
        private (PwrsDriveInfo? Drive, string Path, string Back) ForPathMethod(string? path)
        {
            var (drive, p) = Locate(path, out bool qualified);
            if (qualified && drive != null) return (drive, p, drive.Prefix);
            if (p.Length > 0 && (p[0] == '\\' || p[0] == '/')) return (drive, p.Substring(1), System.IO.Path.DirectorySeparatorChar.ToString());
            return (drive, p, string.Empty);
        }

        /// <summary>
        /// The values of the call running now, as the native provider takes
        /// them before each operation's own arguments: the filter,
        /// the include and exclude patterns, whether -Force was given, and
        /// the credential, or null in place of the empty one PowerShell
        /// gives when none was.
        /// </summary>
        private object?[] Context()
        {
            PSCredential? credential = Credential;
            if (credential != null && (ReferenceEquals(credential, PSCredential.Empty) || string.IsNullOrEmpty(credential.UserName))) credential = null;
            return new object?[] { Filter, Patterns(Include), Patterns(Exclude), Force.IsPresent, credential };
        }

        /// <summary>A call's context with no filter, patterns, -Force or credential.</summary>
        private static readonly object?[] NoContext = { null, Array.Empty<string>(), Array.Empty<string>(), false, null };

        /// <summary>
        /// The call's context without its filter and patterns, for a walk
        /// that must reach every container whatever its name.
        /// </summary>
        private object?[] Unfiltered()
        {
            object?[] context = Context();
            context[0] = null;
            context[1] = Array.Empty<string>();
            context[2] = Array.Empty<string>();
            return context;
        }

        /// <summary>Whether the call has a filter, or include or exclude patterns.</summary>
        private bool Filtered => Filter != null || (Include != null && Include.Count > 0) || (Exclude != null && Exclude.Count > 0);

        private static string[] Patterns(Collection<string>? patterns)
        {
            if (patterns == null || patterns.Count == 0) return Array.Empty<string>();
            var copy = new string[patterns.Count];
            patterns.CopyTo(copy, 0);
            return copy;
        }

        /// <summary>
        /// One operation against a drive's instance, or with no instance
        /// when the path named no drive of this provider, with the
        /// call's context.
        /// </summary>
        private object?[] CallOn(PwrsDriveInfo? drive, uint op, params object?[] args) => CallWith(drive, Context(), op, args);

        /// <summary><see cref="CallOn"/> with the context given.</summary>
        private object?[] CallWith(PwrsDriveInfo? drive, object?[] context, uint op, object?[] args)
        {
            if (drive == null) return Invoke(Module, ProviderId, op, IntPtr.Zero, context, args, TargetOf(drive, args));
            drive.Gate.Enter(false);
            try
            {
                return Invoke(Module, ProviderId, op, drive.Instance, context, args, TargetOf(drive, args));
            }
            finally
            {
                drive.Gate.Leave();
            }
        }

        /// <summary>
        /// The target of an error from an operation on <paramref name="args"/>
        /// when the native provider gives none: the path the operation is
        /// on, with the drive's prefix put back, as the user writes it, or
        /// the drive's name for an operation that takes no path.
        /// </summary>
        private static string? TargetOf(PwrsDriveInfo? drive, object?[] args) => args.Length > 0 && args[0] is string path ? Full(drive, path) : drive?.Name;

        /// <summary>
        /// One operation between two drives: the source drive's instance is
        /// the operation's own, and the destination's goes in as the second
        /// argument. The operation runs inside both drives' gates, entered
        /// in the drives' order.
        /// </summary>
        private object?[] CallBetween(PwrsDriveInfo from, PwrsDriveInfo to, uint op, object?[] args)
        {
            PwrsDriveInfo first = from.Order < to.Order ? from : to;
            PwrsDriveInfo second = ReferenceEquals(first, from) ? to : from;
            first.Gate.Enter(false);
            try
            {
                second.Gate.Enter(false);
                try
                {
                    args[1] = (long)to.Instance;
                    return Invoke(Module, ProviderId, op, from.Instance, Context(), args, TargetOf(from, args));
                }
                finally
                {
                    second.Gate.Leave();
                }
            }
            finally
            {
                first.Gate.Leave();
            }
        }

        /// <summary>
        /// Runs one operation in the native provider: the context's values
        /// go first, then the operation's own arguments. An error a
        /// provider method returned comes back as its cells and is thrown
        /// as the exception <see cref="ProviderError"/> makes, with
        /// <paramref name="target"/> for an error that has no target.
        /// </summary>
        private static unsafe object?[] Invoke(NativeModule module, uint providerId, uint op, IntPtr instance, object?[] context, object?[] args, object? target)
        {
            var all = new object?[context.Length + args.Length];
            context.CopyTo(all, 0);
            args.CopyTo(all, context.Length);
            IntPtr argsHandle = GCHandle.ToIntPtr(GCHandle.Alloc(all));
            IntPtr result = IntPtr.Zero;
            IntPtr err = IntPtr.Zero;
            int status = module.ProviderInvoke(providerId, op, instance, argsHandle, &result, &err);
            GCHandle.FromIntPtr(argsHandle).Free();
            object? errObj = Native.TakeErr(err);
            if (status != Native.Ok)
            {
                if (errObj is object?[] cells) throw PwrsException.FromCells(cells, target);
                throw errObj as Exception ?? new PwrsException(errObj as string ?? $"provider op {op} failed with status {status}");
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
                Invoke(module, providerId, DropDriveOp, instance, NoContext, Array.Empty<object?>(), null);
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

        /// <summary>Writes an item row the native provider returned for an item on <paramref name="drive"/>.</summary>
        private void WriteRow(PwrsDriveInfo? drive, object? row)
        {
            object?[] cells = Row(row);
            if (cells.Length < 3) throw new PwrsException($"an item row needs [path, value, isContainer]; got {cells.Length} cells");
            WriteItemObject(cells[1], Full(drive, Str(cells[0])), Bool(cells[2]));
        }

        private PwrsDriveInfo DriveFromRow(object? row, string description, PSCredential? credential)
        {
            object?[] c = Row(row);
            if (c.Length < 3) throw new PwrsException($"a drive row needs [name, root, instance]; got {c.Length} cells");
            return new PwrsDriveInfo(Str(c[0]), ProviderInfo, Str(c[1]), description, credential, Module, ProviderId, Ptr(c[2]));
        }

        /// <summary>
        /// Refuses an operation between two drives when either path named
        /// no drive of this provider, since the operation needs both
        /// instances.
        /// </summary>
        private static void BothDrives(PwrsDriveInfo? from, PwrsDriveInfo? to, string fromPath, string toPath)
        {
            if (from == null || to == null)
            {
                throw new PwrsException($"'{fromPath}' and '{toPath}' must each be on a drive of this provider");
            }
        }

        // ---- drives ----
        protected override Collection<PSDriveInfo> InitializeDefaultDrives()
        {
            var drives = new Collection<PSDriveInfo>();
            foreach (object? r in CallOn(null, InitDefaultDrivesOp)) drives.Add(DriveFromRow(r, string.Empty, null));
            return drives;
        }

        protected override PSDriveInfo NewDrive(PSDriveInfo drive)
        {
            object?[] rows = CallOn(null, NewDriveOp, drive.Name, drive.Root);
            if (rows.Length == 0) throw new PwrsException("new_drive returned no drive row");
            return DriveFromRow(rows[0], drive.Description, drive.Credential);
        }

        /// <summary>
        /// Runs remove_drive on the instance and frees it; an error
        /// leaves the instance with the drive and propagates.
        /// </summary>
        protected override PSDriveInfo RemoveDrive(PSDriveInfo drive)
        {
            if (drive is PwrsDriveInfo d)
            {
                d.Gate.Enter(false);
                try
                {
                    IntPtr instance = d.Instance;
                    if (instance != IntPtr.Zero)
                    {
                        Invoke(Module, ProviderId, RemoveDriveOp, instance, Context(), Array.Empty<object?>(), d.Name);
                        d.Take();
                    }
                }
                finally
                {
                    d.Gate.Leave();
                }
            }
            return drive;
        }

        // ---- items ----
        protected override bool IsValidPath(string path)
        {
            var (drive, p) = Route(path);
            return Bool(First(CallOn(drive, IsValidPathOp, p), true));
        }

        protected override bool ItemExists(string path)
        {
            var (drive, p) = Route(path);
            return Bool(First(CallOn(drive, ItemExistsOp, p), false));
        }

        protected override bool IsItemContainer(string path)
        {
            var (drive, p) = Route(path);
            return Bool(First(CallOn(drive, IsItemContainerOp, p), false));
        }

        protected override void GetItem(string path)
        {
            var (drive, p) = Route(path);
            foreach (object? r in CallOn(drive, GetItemOp, p)) WriteRow(drive, r);
        }

        protected override void SetItem(string path, object value)
        {
            if (!ShouldProcess($"Item: {path}", "Set Item")) return;
            var (drive, p) = Route(path);
            foreach (object? r in CallOn(drive, SetItemOp, p, value)) WriteRow(drive, r);
        }

        protected override void ClearItem(string path)
        {
            if (!ShouldProcess($"Item: {path}", "Clear Item")) return;
            var (drive, p) = Route(path);
            CallOn(drive, ClearItemOp, p);
        }

        protected override void InvokeDefaultAction(string path)
        {
            if (!ShouldProcess($"Item: {path}", "Invoke Item")) return;
            var (drive, p) = Route(path);
            CallOn(drive, InvokeDefaultActionOp, p);
        }

        /// <summary>
        /// For a provider that declares ExpandWildcards: the paths a path
        /// with wildcards names, from the native provider's expand_path,
        /// each with the drive's prefix put back. PowerShell then runs the
        /// command once for each.
        /// </summary>
        protected override string[] ExpandPath(string path)
        {
            var (drive, p) = Route(path);
            object?[] rows = CallOn(drive, ExpandPathOp, p);
            var paths = new string[rows.Length];
            for (int i = 0; i < rows.Length; i++) paths[i] = Full(drive, Str(rows[i]));
            return paths;
        }

        // ---- containers ----
        protected override void GetChildItems(string path, bool recurse)
        {
            var (drive, p) = Route(path);
            foreach (object? r in CallOn(drive, GetChildItemsOp, p, recurse)) WriteRow(drive, r);
        }

        /// <summary>
        /// Get-ChildItem -Depth: the children of the item at the path, then,
        /// for each child that is a container, that container's children,
        /// down to <paramref name="depth"/> levels below the first, so 0
        /// writes the children alone. Each container's children are written
        /// together, before the walk goes into the first of them. With a
        /// filter or patterns, the children written are those the native
        /// provider gives for the call, and the containers walked into come
        /// from a second listing without them, so a container whose own
        /// name does not match is walked too. Without a depth, recurse has
        /// the native provider give the whole tree.
        /// </summary>
        protected override void GetChildItems(string path, bool recurse, uint depth)
        {
            if (!recurse || depth == uint.MaxValue)
            {
                GetChildItems(path, recurse);
                return;
            }
            var (drive, p) = Route(path);
            bool filtered = Filtered;
            var pending = new Stack<(string Path, uint Levels)>();
            pending.Push((p, depth));
            while (pending.Count > 0 && !Stopping)
            {
                var (next, levels) = pending.Pop();
                var containers = new List<string>();
                foreach (object? r in CallOn(drive, GetChildItemsOp, next, false))
                {
                    WriteRow(drive, r);
                    object?[] cells = Row(r);
                    if (!filtered && Bool(cells[2])) containers.Add(Str(cells[0]));
                }
                if (levels == 0) continue;
                if (filtered)
                {
                    foreach (object? r in CallWith(drive, Unfiltered(), GetChildItemsOp, new object?[] { next, false }))
                    {
                        object?[] cells = Row(r);
                        if (cells.Length >= 3 && Bool(cells[2])) containers.Add(Str(cells[0]));
                    }
                }
                for (int i = containers.Count - 1; i >= 0; i--) pending.Push((containers[i], levels - 1));
            }
        }

        /// <summary>
        /// The names of the children of the item at the path. PowerShell
        /// passes ReturnAllContainers when it walks into every container
        /// whatever the filter, so with a filter or patterns the names the
        /// native provider gives for the call are followed by each container
        /// they leave out, from a listing without the filter and patterns.
        /// </summary>
        protected override void GetChildNames(string path, ReturnContainers returnContainers)
        {
            var (drive, p) = Route(path);
            var written = new HashSet<string>(StringComparer.Ordinal);
            foreach (object? r in CallOn(drive, GetChildNamesOp, p))
            {
                string name = Str(r);
                written.Add(name);
                WriteItemObject(name, name, false);
            }
            if (returnContainers != ReturnContainers.ReturnAllContainers || !Filtered) return;
            foreach (object? r in CallWith(drive, Unfiltered(), GetChildItemsOp, new object?[] { p, false }))
            {
                object?[] cells = Row(r);
                if (cells.Length < 3) throw new PwrsException($"an item row needs [path, value, isContainer]; got {cells.Length} cells");
                if (!Bool(cells[2])) continue;
                string name = GetChildName(Full(drive, Str(cells[0])));
                if (written.Add(name)) WriteItemObject(name, name, true);
            }
        }

        /// <summary>
        /// Whether the item at the path has children, whatever the call's
        /// filter and patterns, as on a FileSystem drive. PowerShell calls
        /// this with the filter set before it lists a level of a wildcard
        /// path, and leaves the level out when it returns false.
        /// </summary>
        protected override bool HasChildItems(string path)
        {
            var (drive, p) = Route(path);
            return Bool(First(CallWith(drive, Unfiltered(), HasChildItemsOp, new object?[] { p }), false));
        }

        protected override void NewItem(string path, string itemTypeName, object newItemValue)
        {
            if (!ShouldProcess($"Item: {path}", "New Item")) return;
            var (drive, p) = Route(path);
            foreach (object? r in CallOn(drive, NewItemOp, p, itemTypeName ?? string.Empty, newItemValue)) WriteRow(drive, r);
        }

        protected override void RemoveItem(string path, bool recurse)
        {
            if (!ShouldProcess($"Item: {path}", "Remove Item")) return;
            var (drive, p) = Route(path);
            CallOn(drive, RemoveItemOp, p, recurse);
        }

        protected override void RenameItem(string path, string newName)
        {
            if (!ShouldProcess($"Item: {path} Destination: {newName}", "Rename Item")) return;
            var (drive, p) = Route(path);
            foreach (object? r in CallOn(drive, RenameItemOp, p, newName)) WriteRow(drive, r);
        }

        /// <summary>
        /// A copy within one drive runs copy_item; a copy to another drive
        /// of this provider runs copy_item_to, with both drives' instances.
        /// </summary>
        protected override void CopyItem(string path, string copyPath, bool recurse)
        {
            if (!ShouldProcess($"Item: {path} Destination: {copyPath}", "Copy Item")) return;
            var (from, p) = Route(path);
            var (to, dest) = Route(copyPath);
            if (ReferenceEquals(from, to))
            {
                foreach (object? r in CallOn(from, CopyItemOp, p, dest, recurse)) WriteRow(from, r);
                return;
            }
            BothDrives(from, to, path, copyPath);
            foreach (object? r in CallBetween(from!, to!, CopyItemToOp, new object?[] { p, null, dest, recurse })) WriteRow(to, r);
        }

        /// <summary>
        /// A move within one drive runs move_item; a move to another drive
        /// of this provider runs move_item_to, with both drives' instances.
        /// Each writes the item at its new path.
        /// </summary>
        protected override void MoveItem(string path, string destination)
        {
            if (!ShouldProcess($"Item: {path} Destination: {destination}", "Move Item")) return;
            var (from, p) = Route(path);
            var (to, dest) = Route(destination);
            if (ReferenceEquals(from, to))
            {
                foreach (object? r in CallOn(from, MoveItemOp, p, dest)) WriteRow(from, r);
                return;
            }
            BothDrives(from, to, path, destination);
            foreach (object? r in CallBetween(from!, to!, MoveItemToOp, new object?[] { p, null, dest })) WriteRow(to, r);
        }

        // ---- navigation ----

        /// <summary>The last segment's name; a name has no drive prefix.</summary>
        protected override string GetChildName(string path)
        {
            var (drive, p) = Route(path);
            return Str(First(CallOn(drive, GetChildNameOp, p), p));
        }

        /// <summary>
        /// The parent, written the way the path was: with the drive's
        /// prefix or the leading separator it came with. The drive's root
        /// has no parent, so it gives the empty string, and a path whose
        /// native parent is empty has the root for a parent, so a walk up
        /// from any path ends at the root.
        /// </summary>
        protected override string GetParentPath(string path, string root)
        {
            var (drive, p, back) = ForPathMethod(path);
            string nativeRoot = root ?? string.Empty;
            if (drive != null && Strip(drive, nativeRoot, out string rootRest)) nativeRoot = rootRest;
            string trimmed = p.TrimEnd('\\', '/');
            if (trimmed.Length == 0 || string.Equals(trimmed, nativeRoot.TrimEnd('\\', '/'), StringComparison.OrdinalIgnoreCase)) return string.Empty;
            return back + Str(First(CallOn(drive, GetParentPathOp, p, nativeRoot), string.Empty));
        }

        /// <summary>
        /// The child joined to the parent, written the way the parent was:
        /// with the drive's prefix or the leading separator it came with.
        /// An empty parent leaves the child as it is.
        /// </summary>
        protected override string MakePath(string parent, string child)
        {
            string c = child ?? string.Empty;
            if (string.IsNullOrEmpty(parent)) return c;
            var (drive, p, back) = ForPathMethod(parent);
            if (drive != null && Strip(drive, c, out string childRest)) c = childRest;
            return back + Str(First(CallOn(drive, MakePathOp, p, c), c));
        }

        /// <summary>
        /// The relative path the native provider returns, or, when it
        /// returns none, the one PowerShell works out from the other path
        /// methods.
        /// </summary>
        protected override string NormalizeRelativePath(string path, string basePath)
        {
            var (drive, p) = Route(path);
            var (_, b) = Route(basePath);
            object?[] r = CallOn(drive, NormalizeRelativePathOp, p, b);
            return r.Length > 0 ? Str(r[0]) : base.NormalizeRelativePath(path, basePath);
        }

        // ---- content ----
        /// <summary>
        /// Each result as the module returned it, the PSObject a wrapped
        /// one comes in included, so the properties that live on the
        /// wrapper, a deserialized object's among them, reach the reader.
        /// </summary>
        public IContentReader GetContentReader(string path)
        {
            var (drive, p) = Route(path);
            var lines = new List<object?>();
            foreach (object? r in CallOn(drive, GetContentOp, p)) lines.Add(r);
            return new ListReader(lines);
        }

        /// <summary>
        /// A writer that passes each batch to the native provider as it is
        /// written, so an error reaches the command that wrote it.
        /// </summary>
        public IContentWriter GetContentWriter(string path) => new AppendWriter(this, path);

        /// <summary>
        /// Clear-Content, and Set-Content on an item that exists. Set-Content
        /// runs its own ShouldProcess for setting the content before this
        /// one runs for clearing it, as on a FileSystem drive.
        /// </summary>
        public void ClearContent(string path)
        {
            if (!ShouldProcess($"Item: {path}", "Clear Content")) return;
            var (drive, p) = Route(path);
            CallOn(drive, ClearContentOp, p);
        }

        public object GetContentReaderDynamicParameters(string path) => null!;
        public object GetContentWriterDynamicParameters(string path) => null!;
        public object ClearContentDynamicParameters(string path) => null!;

        internal void WriteContent(string path, IList content)
        {
            var (drive, p) = Route(path);
            var args = new object?[content.Count + 1];
            args[0] = p;
            content.CopyTo(args, 1);
            CallOn(drive, WriteContentOp, args);
        }

        // ---- item properties ----
        // The native provider takes and returns properties as the cells
        // [name, value, name, value, ...]. Each change runs under
        // ShouldProcess, with the texts the registry provider uses.

        /// <summary>
        /// Writes the properties the native provider returns for the names
        /// given, or for every property when none is, as one object;
        /// nothing when it returns none.
        /// </summary>
        public void GetProperty(string path, Collection<string>? providerSpecificPickList)
        {
            var (drive, p) = Route(path);
            WriteProperties(CallOn(drive, GetPropertyOp, WithNames(p, providerSpecificPickList)), path);
        }

        /// <summary>
        /// Sets each property of the object given that ShouldProcess
        /// passes, and writes them as the native provider now has them.
        /// </summary>
        public void SetProperty(string path, PSObject propertyValue)
        {
            if (propertyValue == null) throw new ArgumentNullException(nameof(propertyValue));
            var (drive, p) = Route(path);
            var args = new List<object?> { p };
            foreach (PSPropertyInfo property in propertyValue.Properties)
            {
                if (!ShouldProcess($"Item: {path} Property: {property.Name}", "Set Property")) continue;
                args.Add(property.Name);
                args.Add(property.Value);
            }
            if (args.Count > 1) WriteProperties(CallOn(drive, SetPropertyOp, args.ToArray()), path);
        }

        /// <summary>
        /// Clears each property named that ShouldProcess passes, and writes
        /// them as the native provider now has them.
        /// </summary>
        public void ClearProperty(string path, Collection<string> propertyToClear)
        {
            var (drive, p) = Route(path);
            var args = new List<object?> { p };
            if (propertyToClear != null)
            {
                foreach (string name in propertyToClear)
                {
                    if (ShouldProcess($"Item: {path} Property: {name}", "Clear Property")) args.Add(name);
                }
            }
            if (args.Count > 1) WriteProperties(CallOn(drive, ClearPropertyOp, args.ToArray()), path);
        }

        public object GetPropertyDynamicParameters(string path, Collection<string>? providerSpecificPickList) => null!;
        public object SetPropertyDynamicParameters(string path, PSObject propertyValue) => null!;
        public object ClearPropertyDynamicParameters(string path, Collection<string> propertyToClear) => null!;

        public void NewProperty(string path, string propertyName, string propertyTypeName, object? value)
        {
            if (!ShouldProcess($"Item: {path} Property: {propertyName}", "New Property")) return;
            var (drive, p) = Route(path);
            WriteProperties(CallOn(drive, NewPropertyOp, p, propertyName, propertyTypeName ?? string.Empty, value), path);
        }

        public void RemoveProperty(string path, string propertyName)
        {
            if (!ShouldProcess($"Item: {path} Property: {propertyName}", "Remove Property")) return;
            var (drive, p) = Route(path);
            CallOn(drive, RemovePropertyOp, p, propertyName);
        }

        public void RenameProperty(string path, string sourceProperty, string destinationProperty)
        {
            if (!ShouldProcess($"Item: {path} SourceProperty: {sourceProperty} DestinationProperty: {destinationProperty}", "Rename Property")) return;
            var (drive, p) = Route(path);
            WriteProperties(CallOn(drive, RenamePropertyOp, p, sourceProperty, destinationProperty), path);
        }

        /// <summary>
        /// Writes the copy at the destination item. A copy to another drive
        /// of this provider runs copy_property_to, with both drives'
        /// instances.
        /// </summary>
        public void CopyProperty(string sourcePath, string sourceProperty, string destinationPath, string destinationProperty)
        {
            if (!ShouldProcess($"Item: {sourcePath} SourceProperty: {sourceProperty} DestinationItem: {destinationPath} DestinationProperty: {destinationProperty}", "Copy Property")) return;
            var (from, p) = Route(sourcePath);
            var (to, dest) = Route(destinationPath);
            if (ReferenceEquals(from, to))
            {
                WriteProperties(CallOn(from, CopyPropertyOp, p, sourceProperty, dest, destinationProperty), destinationPath);
                return;
            }
            BothDrives(from, to, sourcePath, destinationPath);
            WriteProperties(CallBetween(from!, to!, CopyPropertyToOp, new object?[] { p, null, sourceProperty, dest, destinationProperty }), destinationPath);
        }

        /// <summary>
        /// Writes the property at the destination item. A move to another
        /// drive of this provider runs move_property_to, with both drives'
        /// instances.
        /// </summary>
        public void MoveProperty(string sourcePath, string sourceProperty, string destinationPath, string destinationProperty)
        {
            if (!ShouldProcess($"Item: {sourcePath} SourceProperty: {sourceProperty} DestinationItem: {destinationPath} DestinationProperty: {destinationProperty}", "Move Property")) return;
            var (from, p) = Route(sourcePath);
            var (to, dest) = Route(destinationPath);
            if (ReferenceEquals(from, to))
            {
                WriteProperties(CallOn(from, MovePropertyOp, p, sourceProperty, dest, destinationProperty), destinationPath);
                return;
            }
            BothDrives(from, to, sourcePath, destinationPath);
            WriteProperties(CallBetween(from!, to!, MovePropertyToOp, new object?[] { p, null, sourceProperty, dest, destinationProperty }), destinationPath);
        }

        public object NewPropertyDynamicParameters(string path, string propertyName, string propertyTypeName, object? value) => null!;
        public object RemovePropertyDynamicParameters(string path, string propertyName) => null!;
        public object RenamePropertyDynamicParameters(string path, string sourceProperty, string destinationProperty) => null!;
        public object CopyPropertyDynamicParameters(string sourcePath, string sourceProperty, string destinationPath, string destinationProperty) => null!;
        public object MovePropertyDynamicParameters(string sourcePath, string sourceProperty, string destinationPath, string destinationProperty) => null!;

        /// <summary>The path, then each name, as one operation's arguments.</summary>
        private static object?[] WithNames(string path, Collection<string>? names)
        {
            var args = new List<object?> { path };
            if (names != null)
            {
                foreach (string name in names) args.Add(name);
            }
            return args.ToArray();
        }

        /// <summary>
        /// Writes the cells [name, value, name, value, ...] as one object
        /// with those properties, for the item at the path; nothing when
        /// there are none.
        /// </summary>
        private void WriteProperties(object?[] cells, string path)
        {
            if (cells.Length == 0) return;
            if ((cells.Length & 1) != 0) throw new PwrsException($"properties come as name and value pairs; got {cells.Length} cells");
            var properties = new PSObject();
            for (int i = 0; i < cells.Length; i += 2) properties.Properties.Add(new PSNoteProperty(Str(cells[i]), cells[i + 1]));
            WritePropertyObject(properties, path);
        }

        // ---- security descriptors ----
        // The native provider takes and returns a descriptor as SDDL text,
        // with the kind of object it describes. Each kind is built as the
        // .NET object whose rights PowerShell shows by name, where there is
        // one.

        /// <summary>
        /// Writes the descriptor the native provider returns for the item,
        /// covering the sections given.
        /// </summary>
        public void GetSecurityDescriptor(string path, AccessControlSections includeSections)
        {
            var (drive, p) = Route(path);
            WriteSecurityDescriptorObject(Descriptor(CallOn(drive, GetSecurityDescriptorOp, p, (int)includeSections)), path);
        }

        /// <summary>
        /// Passes the native provider each part the descriptor given has, as
        /// SDDL text, and writes that descriptor. Set-Acl runs ShouldProcess
        /// before this call, so this runs no second one.
        /// </summary>
        public void SetSecurityDescriptor(string path, ObjectSecurity securityDescriptor)
        {
            if (securityDescriptor == null) throw new ArgumentNullException(nameof(securityDescriptor));
            var (drive, p) = Route(path);
            CallOn(drive, SetSecurityDescriptorOp, p, securityDescriptor.GetSecurityDescriptorSddlForm(AccessControlSections.All));
            WriteSecurityDescriptorObject(securityDescriptor, path);
        }

        /// <summary>An empty descriptor of the kind the native provider gives the item.</summary>
        public ObjectSecurity NewSecurityDescriptorFromPath(string path, AccessControlSections includeSections)
        {
            var (drive, p) = Route(path);
            object?[] row = CallOn(drive, GetSecurityDescriptorOp, p, (int)includeSections);
            if (row.Length < 2) throw new PwrsException($"a security descriptor row needs [sddl, kind]; got {row.Length} cells");
            return Empty(Int(row[1]));
        }

        /// <summary>An empty descriptor of the kind named: File, Directory, RegistryKey or Other.</summary>
        public ObjectSecurity NewSecurityDescriptorOfType(string type, AccessControlSections includeSections)
        {
            switch ((type ?? string.Empty).ToUpperInvariant())
            {
                case "FILE": return Empty(FileKind);
                case "DIRECTORY": return Empty(DirectoryKind);
                case "REGISTRYKEY": return Empty(RegistryKeyKind);
                case "OTHER": return Empty(OtherKind);
                default: throw new ArgumentException($"there is no security descriptor kind '{type}'; the kinds are File, Directory, RegistryKey and Other", nameof(type));
            }
        }

        /// <summary>The descriptor a [sddl, kind] row gives.</summary>
        private static ObjectSecurity Descriptor(object?[] row)
        {
            if (row.Length < 2) throw new PwrsException($"a security descriptor row needs [sddl, kind]; got {row.Length} cells");
            ObjectSecurity descriptor = Empty(Int(row[1]));
            string sddl = Str(row[0]);
            if (sddl.Length != 0) descriptor.SetSecurityDescriptorSddlForm(sddl);
            return descriptor;
        }

        private static ObjectSecurity Empty(int kind)
        {
            switch (kind)
            {
                case FileKind: return new FileSecurity();
                case DirectoryKind: return new DirectorySecurity();
                case RegistryKeyKind: return new RegistrySecurity();
                case OtherKind: return new ItemSecurity();
                default: throw new PwrsException($"there is no security descriptor kind {kind}");
            }
        }

        private static int Int(object? o) => LanguagePrimitives.ConvertTo<int>(o is PSObject ps ? ps.BaseObject : o);

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

        /// <summary>
        /// Writes each batch at the end of the item. Add-Content's move to
        /// the end is where writes go already; any other move is refused.
        /// Nothing is buffered, so closing sends nothing.
        /// </summary>
        private sealed class AppendWriter : IContentWriter
        {
            private readonly ProviderBase _p;
            private readonly string _path;
            public AppendWriter(ProviderBase p, string path) { _p = p; _path = path; }
            public IList Write(IList content)
            {
                _p.WriteContent(_path, content);
                return content;
            }
            public void Seek(long offset, System.IO.SeekOrigin origin)
            {
                if (offset != 0 || origin != System.IO.SeekOrigin.End)
                {
                    throw new NotSupportedException("a PWRS provider's content writer only appends, so it moves only to the end");
                }
            }
            public void Close() { }
            public void Dispose() { }
        }
    }
}
