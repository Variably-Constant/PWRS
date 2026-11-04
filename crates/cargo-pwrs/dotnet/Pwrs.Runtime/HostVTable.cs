using System;
using System.Collections;
using System.Collections.ObjectModel;
using System.Management.Automation;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Security;

namespace Pwrs
{
    /// <summary>
    /// Builds the function table passed to the native module. Layout
    /// mirrors pwrs_sys::HostVTable field for field: an 8-byte header
    /// (size, version) then one pointer per entry in declaration
    /// order. The table is allocated once in unmanaged memory and
    /// never freed. On net10.0 entries are UnmanagedCallersOnly
    /// statics; on netstandard2.0 they are delegates stored in the Roots
    /// array, so the garbage collector never frees them.
    ///
    /// On net10.0 every module has a copy of this assembly of its own,
    /// so <see cref="Pointer"/> is passed to one module. On
    /// netstandard2.0 every module in the process uses one copy, and
    /// each module is passed a <c>ModuleTable</c>: this table with
    /// factory_new, proxy_enter, proxy_enter_shared, helper_path,
    /// native_file_path and library_folder working for that module alone.
    /// </summary>
    public static unsafe class HostVTable
    {
        /// Function-pointer slots, matching the native HostVTable field
        /// count exactly so the size header equals size_of::&lt;HostVTable&gt;().
        public const int SlotCount = 116;

#if !NET
        private static readonly Delegate[] Roots = new Delegate[SlotCount];
        private static int _rootSlot;

        /// <summary>
        /// The slot factory_new occupies, recorded while Build writes it
        /// and overwritten in each <see cref="ModuleTable"/>.
        /// </summary>
        private static int _factoryNewSlot;

        /// <summary>The slot proxy_enter occupies, overwritten the same way.</summary>
        private static int _proxyEnterSlot;

        /// <summary>The slot proxy_enter_shared occupies, overwritten the same way.</summary>
        private static int _proxyEnterSharedSlot;

        /// <summary>The slot helper_path occupies, overwritten the same way.</summary>
        private static int _helperPathSlot;

        /// <summary>The slot native_file_path occupies, overwritten the same way.</summary>
        private static int _nativeFilePathSlot;

        /// <summary>The slot library_folder occupies, overwritten the same way.</summary>
        private static int _libraryFolderSlot;

        /// <summary>The slot object_event_subscribe occupies, overwritten the same way.</summary>
        private static int _objectEventSubscribeSlot;

        private static IntPtr Root(Delegate d)
        {
            Roots[_rootSlot++] = d;
            return Marshal.GetFunctionPointerForDelegate(d);
        }
#endif

        /// <summary>Declared after Roots: static initializers run in textual order.</summary>
        public static readonly IntPtr Pointer = Build();

        private static IntPtr Build()
        {
            int bytes = 8 + SlotCount * IntPtr.Size;
            IntPtr table = Marshal.AllocHGlobal(bytes);
            Marshal.WriteInt32(table, 0, bytes);
            Marshal.WriteInt32(table, 4, (int)Native.AbiVersion);
            for (int i = 0; i < SlotCount; i++) Marshal.WriteIntPtr(table, 8 + i * IntPtr.Size, IntPtr.Zero);

            int slot = 0;
            void Set(IntPtr fn) => Marshal.WriteIntPtr(table, 8 + slot++ * IntPtr.Size, fn);

#if NET
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, void>)&Entries.FreeHandle);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr>)&Entries.CloneHandle);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, byte, IntPtr*, int>)&Entries.WriteObject);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, PsStr16, uint, IntPtr, byte, IntPtr*, int>)&Entries.WriteError);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, uint, PsStr16, IntPtr*, int>)&Entries.WriteStream);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, int, PsStr16, PsStr16, int, IntPtr*, int>)&Entries.WriteProgress);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, PsStr16, byte*, IntPtr*, int>)&Entries.ShouldProcess);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, PsStr16, byte*, IntPtr*, int>)&Entries.ShouldContinue);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, IntPtr*, IntPtr*, int>)&Entries.GetParameter);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, byte*, int>)&Entries.ParameterIsBound);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<PsStr16, IntPtr>)&Entries.StringNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, ushort*, nuint, nuint*, IntPtr*, int>)&Entries.StringRead);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<long, IntPtr>)&Entries.I64New);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, long*, IntPtr*, int>)&Entries.I64Read);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<double, IntPtr>)&Entries.F64New);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, double*, IntPtr*, int>)&Entries.F64Read);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<byte, IntPtr>)&Entries.BoolNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, byte*, IntPtr*, int>)&Entries.BoolRead);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, nuint*, IntPtr*, int>)&Entries.ArrayLen);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, nuint, IntPtr*, IntPtr*, int>)&Entries.ArrayGet);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<uint, nuint, IntPtr*, IntPtr*, int>)&Entries.ArrayNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, nuint, IntPtr, IntPtr*, int>)&Entries.ArraySet);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsPinned*, IntPtr*, int>)&Entries.ArrayPin);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<PsPinned, void>)&Entries.ArrayUnpin);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<PsStr16, IntPtr>)&Entries.PsObjectNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, IntPtr, IntPtr*, int>)&Entries.PsObjectAddNote);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, IntPtr*, IntPtr*, int>)&Entries.PsObjectGetProperty);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, IntPtr*, IntPtr*, int>)&Entries.GetVariable);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, IntPtr, IntPtr*, int>)&Entries.SetVariable);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, byte, IntPtr*, IntPtr*, int>)&Entries.ResolvePath);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, IntPtr, IntPtr*, IntPtr*, int>)&Entries.InvokeScriptBlock);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, IntPtr*, IntPtr*, int>)&Entries.DynGet);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, IntPtr, IntPtr*, int>)&Entries.DynSet);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, IntPtr, IntPtr*, IntPtr*, int>)&Entries.DynCall);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<PsStr16, PsStr16, IntPtr, IntPtr*, IntPtr*, int>)&Entries.DynCallStatic);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<PsStr16, IntPtr, IntPtr*, IntPtr*, int>)&Entries.DynNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<uint, void*, IntPtr*, IntPtr*, int>)&Entries.FactoryNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<uint, void*, nuint, IntPtr, IntPtr*, IntPtr*, int>)&Entries.MemoryViewNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, ushort*, nuint, nuint*, int>)&Entries.ExceptionDescribe);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, IntPtr*, int>)&Entries.WriteString);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, long, IntPtr*, int>)&Entries.WriteI64);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, double, IntPtr*, int>)&Entries.WriteF64);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, byte, IntPtr*, int>)&Entries.WriteBool);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<ulong, IntPtr>)&Entries.U64New);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, ulong*, IntPtr*, int>)&Entries.U64Read);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<long, byte, IntPtr*, IntPtr*, int>)&Entries.DateTimeNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, long*, byte*, IntPtr*, int>)&Entries.DateTimeRead);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<long, IntPtr>)&Entries.TimeSpanNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, long*, IntPtr*, int>)&Entries.TimeSpanRead);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<byte*, IntPtr>)&Entries.GuidNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, byte*, IntPtr*, int>)&Entries.GuidRead);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<ushort, IntPtr>)&Entries.CharNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, ushort*, IntPtr*, int>)&Entries.CharRead);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<PsStr16, IntPtr*, IntPtr*, int>)&Entries.SecureStringNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, ushort*, nuint, nuint*, IntPtr*, int>)&Entries.SecureStringRead);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, uint*, IntPtr*, int>)&Entries.ArrayElementTag);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, uint, byte*, IntPtr*, int>)&Entries.StreamEnabled);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr*, IntPtr*, int>)&Entries.ReadOnlyTableNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, uint*, IntPtr*, int>)&Entries.ObjectTypeTag);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<sbyte, IntPtr>)&Entries.I8New);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<short, IntPtr>)&Entries.I16New);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<int, IntPtr>)&Entries.I32New);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<byte, IntPtr>)&Entries.U8New);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<ushort, IntPtr>)&Entries.U16New);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<uint, IntPtr>)&Entries.U32New);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<float, IntPtr>)&Entries.F32New);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<int, int, int, int, IntPtr*, IntPtr*, int>)&Entries.DecimalNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, int*, IntPtr*, int>)&Entries.DecimalRead);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<long, short, IntPtr*, IntPtr*, int>)&Entries.DateTimeOffsetNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, long*, short*, IntPtr*, int>)&Entries.DateTimeOffsetRead);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, IntPtr, IntPtr, IntPtr*, IntPtr*, int>)&Entries.InvokeCommand);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<PsStr16, long, IntPtr*, IntPtr*, int>)&Entries.EnumNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, uint, IntPtr*, IntPtr*, int>)&Entries.ProxyEnter);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, byte, void>)&Entries.ProxyExit);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<PsStr16, ushort*, nuint, nuint*, IntPtr*, int>)&Entries.HelperPath);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, uint, IntPtr*, IntPtr*, int>)&Entries.ProxyEnterShared);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, PsStr16, uint, IntPtr, byte, PsStr16, IntPtr*, int>)&Entries.WriteErrorDetails);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<PsStr16, PsStr16, IntPtr>)&Entries.ExceptionNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, int, int, PsStr16, PsStr16, PsStr16, int, int, byte, IntPtr*, int>)&Entries.WriteProgressRecord);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsPinned*, IntPtr*, int>)&Entries.StringPin);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<uint, void*, nuint, byte, byte*, void*, IntPtr, IntPtr*, IntPtr*, int>)&Entries.MemoryViewBorrow);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, uint, IntPtr, IntPtr*, int>)&Entries.TaskSettle);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, IntPtr, IntPtr, IntPtr, IntPtr*, int>)&Entries.EventRaise);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, int, int, byte, IntPtr*, int>)&Entries.WriteHost);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<PsStr16, ushort*, nuint, nuint*, IntPtr*, int>)&Entries.NativeFilePath);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<uint*, byte*, byte*, IntPtr*, int>)&Entries.LifecycleRemoval);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, IntPtr, IntPtr*, int>)&Entries.ObjectEventRaise);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, IntPtr, IntPtr, IntPtr, IntPtr*, IntPtr*, int>)&Entries.ObjectEventSubscribe);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr*, int>)&Entries.ObjectEventUnsubscribe);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, byte, IntPtr*, int>)&Entries.ProxyExitRaising);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, byte, PsStr16, PsStr16, PsStr16, byte*, byte*, IntPtr*, int>)&Entries.ShouldProcessForm);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, PsStr16, byte, byte*, IntPtr*, int>)&Entries.ShouldContinueAll);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, ulong*, ulong*, byte*, IntPtr*, int>)&Entries.Paging);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, ulong, double, IntPtr*, int>)&Entries.WriteTotalCount);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, IntPtr, IntPtr, IntPtr, IntPtr*, IntPtr*, int>)&Entries.JobNew);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, byte, IntPtr*, int>)&Entries.JobWrite);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, uint, PsStr16, IntPtr*, int>)&Entries.JobWriteStream);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, PsStr16, uint, IntPtr, PsStr16, IntPtr*, int>)&Entries.JobWriteError);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, int, int, PsStr16, PsStr16, PsStr16, int, int, byte, IntPtr*, int>)&Entries.JobWriteProgress);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, byte, PsStr16, PsStr16, IntPtr*, int>)&Entries.JobFinish);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr*, IntPtr*, int>)&Entries.TransactionCurrent);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, IntPtr, IntPtr, IntPtr*, int>)&Entries.TransactionEnlist);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, PsStr16, IntPtr, IntPtr*, byte*, IntPtr*, int>)&Entries.SteppableBegin);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, IntPtr, byte, IntPtr*, int>)&Entries.SteppableProcess);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, IntPtr*, int>)&Entries.SteppableEnd);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, IntPtr*, int>)&Entries.SteppableDispose);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr*, IntPtr*, int>)&Entries.WrappedArguments);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr*, byte*, IntPtr*, int>)&Entries.WrappedInput);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, IntPtr, IntPtr, byte*, IntPtr*, int>)&Entries.WrappedBegin);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, byte, IntPtr, IntPtr, IntPtr*, int>)&Entries.WrappedProcess);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, IntPtr, IntPtr*, int>)&Entries.WrappedEnd);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<PsStr16, PsStr16, PsStr16, uint, PsStr16, IntPtr>)&Entries.ExceptionNamed);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, PsStr16, uint, IntPtr, byte, IntPtr*, int>)&Entries.WriteErrorException);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, PsStr16, uint, IntPtr, IntPtr*, int>)&Entries.JobWriteErrorException);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, IntPtr*, int>)&Entries.JobFinishException);
            Set((IntPtr)(delegate* unmanaged[Cdecl]<PsStr16, ushort*, nuint, nuint*, IntPtr*, int>)&Entries.LibraryFolder);
#else
            Set(Root(new FreeHandleFn(Entries.FreeHandle)));
            Set(Root(new CloneHandleFn(Entries.CloneHandle)));
            Set(Root(new WriteObjectFn(Entries.WriteObject)));
            Set(Root(new WriteErrorFn(Entries.WriteError)));
            Set(Root(new WriteStreamFn(Entries.WriteStream)));
            Set(Root(new WriteProgressFn(Entries.WriteProgress)));
            Set(Root(new ShouldFn(Entries.ShouldProcess)));
            Set(Root(new ShouldFn(Entries.ShouldContinue)));
            Set(Root(new HandleNameOutFn(Entries.GetParameter)));
            Set(Root(new ParameterIsBoundFn(Entries.ParameterIsBound)));
            Set(Root(new StringNewFn(Entries.StringNew)));
            Set(Root(new StringReadFn(Entries.StringRead)));
            Set(Root(new I64NewFn(Entries.I64New)));
            Set(Root(new I64ReadFn(Entries.I64Read)));
            Set(Root(new F64NewFn(Entries.F64New)));
            Set(Root(new F64ReadFn(Entries.F64Read)));
            Set(Root(new BoolNewFn(Entries.BoolNew)));
            Set(Root(new BoolReadFn(Entries.BoolRead)));
            Set(Root(new ArrayLenFn(Entries.ArrayLen)));
            Set(Root(new ArrayGetFn(Entries.ArrayGet)));
            Set(Root(new ArrayNewFn(Entries.ArrayNew)));
            Set(Root(new ArraySetFn(Entries.ArraySet)));
            Set(Root(new ArrayPinFn(Entries.ArrayPin)));
            Set(Root(new ArrayUnpinFn(Entries.ArrayUnpin)));
            Set(Root(new StringNewFn(Entries.PsObjectNew)));
            Set(Root(new HandleNameValueFn(Entries.PsObjectAddNote)));
            Set(Root(new HandleNameOutFn(Entries.PsObjectGetProperty)));
            Set(Root(new HandleNameOutFn(Entries.GetVariable)));
            Set(Root(new HandleNameValueFn(Entries.SetVariable)));
            Set(Root(new ResolvePathFn(Entries.ResolvePath)));
            Set(Root(new InvokeScriptBlockFn(Entries.InvokeScriptBlock)));
            Set(Root(new HandleNameOutFn(Entries.DynGet)));
            Set(Root(new HandleNameValueFn(Entries.DynSet)));
            Set(Root(new DynCallFn(Entries.DynCall)));
            Set(Root(new DynCallStaticFn(Entries.DynCallStatic)));
            Set(Root(new DynNewFn(Entries.DynNew)));
            _factoryNewSlot = slot;
            Set(Root(new FactoryNewFn(Entries.FactoryNew)));
            Set(Root(new MemoryViewNewFn(Entries.MemoryViewNew)));
            Set(Root(new ExceptionDescribeFn(Entries.ExceptionDescribe)));
            Set(Root(new WriteStringFn(Entries.WriteString)));
            Set(Root(new WriteI64Fn(Entries.WriteI64)));
            Set(Root(new WriteF64Fn(Entries.WriteF64)));
            Set(Root(new WriteBoolFn(Entries.WriteBool)));
            Set(Root(new U64NewFn(Entries.U64New)));
            Set(Root(new U64ReadFn(Entries.U64Read)));
            Set(Root(new DateTimeNewFn(Entries.DateTimeNew)));
            Set(Root(new DateTimeReadFn(Entries.DateTimeRead)));
            Set(Root(new I64NewFn(Entries.TimeSpanNew)));
            Set(Root(new I64ReadFn(Entries.TimeSpanRead)));
            Set(Root(new GuidNewFn(Entries.GuidNew)));
            Set(Root(new GuidReadFn(Entries.GuidRead)));
            Set(Root(new CharNewFn(Entries.CharNew)));
            Set(Root(new CharReadFn(Entries.CharRead)));
            Set(Root(new SecureStringNewFn(Entries.SecureStringNew)));
            Set(Root(new StringReadFn(Entries.SecureStringRead)));
            Set(Root(new ArrayElementTagFn(Entries.ArrayElementTag)));
            Set(Root(new StreamEnabledFn(Entries.StreamEnabled)));
            Set(Root(new ReadOnlyTableNewFn(Entries.ReadOnlyTableNew)));
            Set(Root(new ArrayElementTagFn(Entries.ObjectTypeTag)));
            Set(Root(new I8NewFn(Entries.I8New)));
            Set(Root(new I16NewFn(Entries.I16New)));
            Set(Root(new I32NewFn(Entries.I32New)));
            Set(Root(new U8NewFn(Entries.U8New)));
            Set(Root(new U16NewFn(Entries.U16New)));
            Set(Root(new U32NewFn(Entries.U32New)));
            Set(Root(new F32NewFn(Entries.F32New)));
            Set(Root(new DecimalNewFn(Entries.DecimalNew)));
            Set(Root(new DecimalReadFn(Entries.DecimalRead)));
            Set(Root(new DateTimeOffsetNewFn(Entries.DateTimeOffsetNew)));
            Set(Root(new DateTimeOffsetReadFn(Entries.DateTimeOffsetRead)));
            Set(Root(new InvokeCommandFn(Entries.InvokeCommand)));
            Set(Root(new EnumNewFn(Entries.EnumNew)));
            _proxyEnterSlot = slot;
            Set(Root(new ProxyEnterFn(Entries.ProxyEnter)));
            Set(Root(new ProxyExitFn(Entries.ProxyExit)));
            _helperPathSlot = slot;
            Set(Root(new HelperPathFn(Entries.HelperPath)));
            _proxyEnterSharedSlot = slot;
            Set(Root(new ProxyEnterFn(Entries.ProxyEnterShared)));
            Set(Root(new WriteErrorDetailsFn(Entries.WriteErrorDetails)));
            Set(Root(new ExceptionNewFn(Entries.ExceptionNew)));
            Set(Root(new WriteProgressRecordFn(Entries.WriteProgressRecord)));
            Set(Root(new ArrayPinFn(Entries.StringPin)));
            Set(Root(new MemoryViewBorrowFn(Entries.MemoryViewBorrow)));
            Set(Root(new TaskSettleFn(Entries.TaskSettle)));
            Set(Root(new EventRaiseFn(Entries.EventRaise)));
            Set(Root(new WriteHostFn(Entries.WriteHost)));
            _nativeFilePathSlot = slot;
            Set(Root(new HelperPathFn(Entries.NativeFilePath)));
            Set(Root(new LifecycleRemovalFn(Entries.LifecycleRemoval)));
            Set(Root(new ObjectEventRaiseFn(Entries.ObjectEventRaise)));
            _objectEventSubscribeSlot = slot;
            Set(Root(new ObjectEventSubscribeFn(Entries.ObjectEventSubscribe)));
            Set(Root(new ObjectEventUnsubscribeFn(Entries.ObjectEventUnsubscribe)));
            Set(Root(new ProxyExitRaisingFn(Entries.ProxyExitRaising)));
            Set(Root(new ShouldProcessFormFn(Entries.ShouldProcessForm)));
            Set(Root(new ShouldContinueAllFn(Entries.ShouldContinueAll)));
            Set(Root(new PagingFn(Entries.Paging)));
            Set(Root(new WriteTotalCountFn(Entries.WriteTotalCount)));
            Set(Root(new JobNewFn(Entries.JobNew)));
            Set(Root(new JobWriteFn(Entries.JobWrite)));
            Set(Root(new JobWriteStreamFn(Entries.JobWriteStream)));
            Set(Root(new JobWriteErrorFn(Entries.JobWriteError)));
            Set(Root(new JobWriteProgressFn(Entries.JobWriteProgress)));
            Set(Root(new JobFinishFn(Entries.JobFinish)));
            Set(Root(new TransactionCurrentFn(Entries.TransactionCurrent)));
            Set(Root(new TransactionEnlistFn(Entries.TransactionEnlist)));
            Set(Root(new SteppableBeginFn(Entries.SteppableBegin)));
            Set(Root(new SteppableProcessFn(Entries.SteppableProcess)));
            Set(Root(new SteppableEndFn(Entries.SteppableEnd)));
            Set(Root(new SteppableDisposeFn(Entries.SteppableDispose)));
            Set(Root(new WrappedArgumentsFn(Entries.WrappedArguments)));
            Set(Root(new WrappedInputFn(Entries.WrappedInput)));
            Set(Root(new WrappedBeginFn(Entries.WrappedBegin)));
            Set(Root(new WrappedProcessFn(Entries.WrappedProcess)));
            Set(Root(new WrappedEndFn(Entries.WrappedEnd)));
            Set(Root(new ExceptionNamedFn(Entries.ExceptionNamed)));
            Set(Root(new WriteErrorExceptionFn(Entries.WriteErrorException)));
            Set(Root(new JobWriteErrorExceptionFn(Entries.JobWriteErrorException)));
            Set(Root(new JobFinishExceptionFn(Entries.JobFinishException)));
            _libraryFolderSlot = slot;
            Set(Root(new HelperPathFn(Entries.LibraryFolder)));
#endif
            return table;
        }

#if !NET
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void FreeHandleFn(IntPtr h);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr CloneHandleFn(IntPtr h);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WriteObjectFn(IntPtr c, IntPtr obj, byte enumerate, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WriteErrorFn(IntPtr c, PsStr16 message, PsStr16 id, uint category, IntPtr target, byte terminating, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WriteStreamFn(IntPtr c, uint kind, PsStr16 text, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WriteProgressFn(IntPtr c, int id, PsStr16 activity, PsStr16 status, int percent, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ShouldFn(IntPtr c, PsStr16 a, PsStr16 b, byte* yes, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int HandleNameOutFn(IntPtr h, PsStr16 name, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int HandleNameValueFn(IntPtr h, PsStr16 name, IntPtr value, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ParameterIsBoundFn(IntPtr c, PsStr16 name, byte* bound);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr StringNewFn(PsStr16 s);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int StringReadFn(IntPtr h, ushort* buf, nuint cap, nuint* len, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr I64NewFn(long v);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int I64ReadFn(IntPtr h, long* v, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr F64NewFn(double v);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int F64ReadFn(IntPtr h, double* v, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr BoolNewFn(byte v);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int BoolReadFn(IntPtr h, byte* v, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ArrayLenFn(IntPtr h, nuint* len, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ArrayGetFn(IntPtr h, nuint i, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ArrayNewFn(uint tag, nuint len, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ArraySetFn(IntPtr h, nuint i, IntPtr value, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ArrayPinFn(IntPtr h, PsPinned* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void ArrayUnpinFn(PsPinned p);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ResolvePathFn(IntPtr c, PsStr16 path, byte literal, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int InvokeScriptBlockFn(IntPtr c, IntPtr block, IntPtr args, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int DynCallFn(IntPtr h, PsStr16 name, IntPtr args, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int DynCallStaticFn(PsStr16 type, PsStr16 name, IntPtr args, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int DynNewFn(PsStr16 type, IntPtr args, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int FactoryNewFn(uint classId, void* fields, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int MemoryViewNewFn(uint tag, void* ptr, nuint len, IntPtr drop, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int MemoryViewBorrowFn(uint tag, void* ptr, nuint len, byte readOnly, byte* refused, void* context, IntPtr release, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int TaskSettleFn(IntPtr source, uint outcome, IntPtr value, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int EventRaiseFn(IntPtr events, PsStr16 sourceIdentifier, IntPtr sender, IntPtr args, IntPtr messageData, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WriteHostFn(IntPtr c, PsStr16 text, int foreground, int background, byte noNewLine, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ExceptionDescribeFn(IntPtr err, ushort* buf, nuint cap, nuint* len);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WriteStringFn(IntPtr c, PsStr16 text, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WriteI64Fn(IntPtr c, long v, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WriteF64Fn(IntPtr c, double v, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WriteBoolFn(IntPtr c, byte v, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr U64NewFn(ulong v);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int U64ReadFn(IntPtr h, ulong* v, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int DateTimeNewFn(long ticks, byte kind, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int DateTimeReadFn(IntPtr h, long* ticks, byte* kind, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr GuidNewFn(byte* bytes);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int GuidReadFn(IntPtr h, byte* bytes, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr CharNewFn(ushort unit);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int CharReadFn(IntPtr h, ushort* unit, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int SecureStringNewFn(PsStr16 text, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ArrayElementTagFn(IntPtr h, uint* tag, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int StreamEnabledFn(IntPtr c, uint kind, byte* enabled, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ReadOnlyTableNewFn(IntPtr source, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr I8NewFn(sbyte v);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr I16NewFn(short v);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr I32NewFn(int v);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr U8NewFn(byte v);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr U16NewFn(ushort v);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr U32NewFn(uint v);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr F32NewFn(float v);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int DecimalNewFn(int lo, int mid, int hi, int flags, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int DecimalReadFn(IntPtr h, int* bits, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int DateTimeOffsetNewFn(long ticks, short offsetMinutes, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int DateTimeOffsetReadFn(IntPtr h, long* ticks, short* offsetMinutes, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int InvokeCommandFn(IntPtr c, PsStr16 name, IntPtr parameters, IntPtr input, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int EnumNewFn(PsStr16 typeName, long value, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ProxyEnterFn(IntPtr obj, uint classId, IntPtr* instance, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void ProxyExitFn(IntPtr obj, byte changed);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int HelperPathFn(PsStr16 name, ushort* buf, nuint cap, nuint* len, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int LifecycleRemovalFn(uint* cause, byte* runspace, byte* hasRunspace, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ObjectEventRaiseFn(IntPtr obj, PsStr16 name, IntPtr args, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ObjectEventSubscribeFn(IntPtr source, PsStr16 name, IntPtr callback, IntPtr context, IntPtr release, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ObjectEventUnsubscribeFn(IntPtr subscription, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ProxyExitRaisingFn(IntPtr obj, byte changed, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ShouldProcessFormFn(IntPtr c, byte form, PsStr16 a, PsStr16 b, PsStr16 caption, byte* yes, byte* reason, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ShouldContinueAllFn(IntPtr c, PsStr16 query, PsStr16 caption, byte impact, byte* yes, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int PagingFn(IntPtr c, ulong* first, ulong* skip, byte* includeTotalCount, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WriteTotalCountFn(IntPtr c, ulong total, double accuracy, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int JobNewFn(IntPtr c, PsStr16 name, IntPtr stop, IntPtr context, IntPtr release, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int JobWriteFn(IntPtr job, IntPtr value, byte enumerate, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int JobWriteStreamFn(IntPtr job, uint kind, PsStr16 text, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int JobWriteErrorFn(IntPtr job, PsStr16 message, PsStr16 id, uint category, IntPtr target, PsStr16 details, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int JobWriteProgressFn(IntPtr job, int id, int parent, PsStr16 activity, PsStr16 status, PsStr16 operation, int percent, int seconds, byte completed, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int JobFinishFn(IntPtr job, byte failed, PsStr16 message, PsStr16 details, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int TransactionCurrentFn(IntPtr c, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int TransactionEnlistFn(IntPtr transaction, IntPtr prepare, IntPtr end, IntPtr context, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int SteppableBeginFn(IntPtr c, PsStr16 name, IntPtr parameters, IntPtr* @out, byte* delegates, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int SteppableProcessFn(IntPtr c, IntPtr steppable, IntPtr input, byte hasInput, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int SteppableEndFn(IntPtr c, IntPtr steppable, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int SteppableDisposeFn(IntPtr c, IntPtr steppable, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WrappedArgumentsFn(IntPtr c, IntPtr* @out, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WrappedInputFn(IntPtr c, IntPtr* @out, byte* has, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WrappedBeginFn(IntPtr c, IntPtr arguments, IntPtr hook, IntPtr context, byte* delegates, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WrappedProcessFn(IntPtr c, IntPtr input, byte hasInput, IntPtr hook, IntPtr context, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WrappedEndFn(IntPtr c, IntPtr hook, IntPtr context, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void DropFn(void* p);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WriteErrorDetailsFn(IntPtr c, PsStr16 message, PsStr16 id, uint category, IntPtr target, byte terminating, PsStr16 details, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr ExceptionNewFn(PsStr16 message, PsStr16 details);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WriteProgressRecordFn(IntPtr c, int id, int parent, PsStr16 activity, PsStr16 status, PsStr16 operation, int percent, int seconds, byte completed, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr ExceptionNamedFn(PsStr16 type, PsStr16 message, PsStr16 id, uint category, PsStr16 details);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int WriteErrorExceptionFn(IntPtr c, IntPtr exception, PsStr16 id, uint category, IntPtr target, byte terminating, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int JobWriteErrorExceptionFn(IntPtr job, IntPtr exception, PsStr16 id, uint category, IntPtr target, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int JobFinishExceptionFn(IntPtr job, IntPtr exception, IntPtr* err);

        /// <summary>
        /// The host table one module's native library receives on .NET
        /// Framework: a copy of <see cref="HostVTable.Pointer"/> whose
        /// factory_new, proxy_enter and proxy_enter_shared use that
        /// module's factories, whose helper_path, native_file_path and
        /// library_folder stage from that module's folder, and whose
        /// object_event_subscribe records a handler on a COM object among
        /// that module's COM event handlers. Class ids are only unique
        /// within a module, and every module in the process uses one copy of
        /// this assembly, so the shared table cannot tell whose class an id
        /// names, whose file a name means or whose handlers to detach when a
        /// module unloads.
        ///
        /// The copy has the addresses of the delegates behind those
        /// seven entries, so the delegates live as long as this object,
        /// which the module's NativeModule stores for the life of the
        /// process. Like the shared table, the copy is never freed.
        /// </summary>
        internal sealed class ModuleTable
        {
            internal readonly IntPtr Pointer;
            private readonly FactoryNewFn _factoryNew;
            private readonly ProxyEnterFn _proxyEnter;
            private readonly ProxyEnterFn _proxyEnterShared;
            private readonly HelperPathFn _helperPath;
            private readonly HelperPathFn _nativeFilePath;
            private readonly HelperPathFn _libraryFolder;
            private readonly ObjectEventSubscribeFn _objectEventSubscribe;

            internal ModuleTable(Factories factories, string moduleRoot, ComHooks comHooks)
            {
                _factoryNew = (classId, fields, @out, err) => Entries.FactoryNewIn(factories, classId, fields, @out, err);
                _proxyEnter = (obj, classId, instance, err) => Entries.ProxyEnterIn(factories, obj, classId, instance, err, true);
                _proxyEnterShared = (obj, classId, instance, err) => Entries.ProxyEnterIn(factories, obj, classId, instance, err, false);
                _helperPath = (name, buf, cap, len, err) => Entries.HelperPathIn(moduleRoot, name, buf, cap, len, err);
                _nativeFilePath = (name, buf, cap, len, err) => Entries.NativeFilePathIn(moduleRoot, name, buf, cap, len, err);
                _libraryFolder = (name, buf, cap, len, err) => Entries.LibraryFolderIn(moduleRoot, name, buf, cap, len, err);
                _objectEventSubscribe = (source, name, callback, context, release, @out, err) => Entries.ObjectEventSubscribeIn(comHooks, source, name, callback, context, release, @out, err);
                int bytes = 8 + SlotCount * IntPtr.Size;
                Pointer = Marshal.AllocHGlobal(bytes);
                Buffer.MemoryCopy((void*)HostVTable.Pointer, (void*)Pointer, bytes, bytes);
                Marshal.WriteIntPtr(Pointer, 8 + _factoryNewSlot * IntPtr.Size, Marshal.GetFunctionPointerForDelegate(_factoryNew));
                Marshal.WriteIntPtr(Pointer, 8 + _proxyEnterSlot * IntPtr.Size, Marshal.GetFunctionPointerForDelegate(_proxyEnter));
                Marshal.WriteIntPtr(Pointer, 8 + _proxyEnterSharedSlot * IntPtr.Size, Marshal.GetFunctionPointerForDelegate(_proxyEnterShared));
                Marshal.WriteIntPtr(Pointer, 8 + _helperPathSlot * IntPtr.Size, Marshal.GetFunctionPointerForDelegate(_helperPath));
                Marshal.WriteIntPtr(Pointer, 8 + _nativeFilePathSlot * IntPtr.Size, Marshal.GetFunctionPointerForDelegate(_nativeFilePath));
                Marshal.WriteIntPtr(Pointer, 8 + _libraryFolderSlot * IntPtr.Size, Marshal.GetFunctionPointerForDelegate(_libraryFolder));
                Marshal.WriteIntPtr(Pointer, 8 + _objectEventSubscribeSlot * IntPtr.Size, Marshal.GetFunctionPointerForDelegate(_objectEventSubscribe));
            }
        }
#endif

        /// <summary>What a memory view throws once the module has revoked it.</summary>
        internal static ObjectDisposedException MemoryRevoked() =>
            new ObjectDisposedException(null, "This memory view has been revoked by the module that made it.");

        /// <summary>What a memory view throws once its memory has been released.</summary>
        internal static ObjectDisposedException MemoryReleased() =>
            new ObjectDisposedException(null, "This memory view has been disposed, and the memory it was over released.");

        /// <summary>
        /// The entries. Each catches every managed exception and reports
        /// it through the status and err out-parameter; a
        /// PipelineStoppedException marks the cmdlet stopped and returns
        /// status 5 so the native side unwinds promptly.
        /// </summary>
#if NET
        [SkipLocalsInit]
#endif
        internal static unsafe class Entries
        {
            [MethodImpl(MethodImplOptions.AggressiveInlining)]
            private static object? Target(IntPtr h) => h == IntPtr.Zero ? null : GCHandle.FromIntPtr(h).Target;

            [MethodImpl(MethodImplOptions.AggressiveInlining)]
            private static object? Base(IntPtr h)
            {
                object? o = Target(h);
                return o is PSObject p ? p.BaseObject : o;
            }

            [MethodImpl(MethodImplOptions.AggressiveInlining)]
            private static IntPtr Alloc(object? o) => o == null ? IntPtr.Zero : GCHandle.ToIntPtr(GCHandle.Alloc(o));

            [MethodImpl(MethodImplOptions.AggressiveInlining)]
            private static RustCmdlet Cmdlet(IntPtr h) => (RustCmdlet)GCHandle.FromIntPtr(h).Target!;

            [MethodImpl(MethodImplOptions.NoInlining)]
            /// <summary>
            /// Returns <paramref name="e"/> through the err out-parameter.
            /// A call made through reflection, a constructor or a static
            /// method, arrives wrapped in a TargetInvocationException, and
            /// the exception inside it is the one returned, so its type
            /// and message say what failed.
            /// </summary>
            private static int Fail(Exception e, IntPtr* err)
            {
                while (e is TargetInvocationException { InnerException: { } inner }) e = inner;
                if (err != null) *err = Native.Capture(e);
                return Native.ErrManagedException;
            }

            [MethodImpl(MethodImplOptions.NoInlining)]
            private static int FailOn(RustCmdlet c, Exception e, IntPtr* err)
            {
                if (e is PipelineStoppedException)
                {
                    c.MarkStopped();
                    return Native.ErrPipelineStopped;
                }
                return Fail(e, err);
            }

            /// <summary>
            /// For a call that writes output or shows a prompt: an
            /// exception the engine raises there is not an error about
            /// the cmdlet's own record but one passing through it, such as
            /// a later command's throw or a prompt no host can show, so
            /// it ends the command as it ends a compiled cmdlet making the
            /// same call. It is stored for Invoke to throw, and returned to
            /// the native side as any failure so the native work stops.
            /// </summary>
            [MethodImpl(MethodImplOptions.NoInlining)]
            private static int FailThrough(RustCmdlet c, Exception e, IntPtr* err)
            {
                if (e is PipelineStoppedException)
                {
                    c.MarkStopped();
                    return Native.ErrPipelineStopped;
                }
                c.HoldPassedThrough(e);
                return Fail(e, err);
            }

            /// <summary>
            /// Refuses a write or a prompt once an exception has passed
            /// through one this phase, returning that exception again, so
            /// a command the engine is ending writes and prompts nothing more.
            /// </summary>
            [MethodImpl(MethodImplOptions.AggressiveInlining)]
            private static bool Refused(RustCmdlet c, IntPtr* err, out int status)
            {
                Exception? passed = c.PassedThrough;
                status = passed == null ? Native.Ok : Fail(passed, err);
                return passed != null;
            }

            [MethodImpl(MethodImplOptions.AggressiveInlining)]
            private static bool OffThread(RustCmdlet c, IntPtr* err)
            {
                if (c.OnPipelineThread) return false;
                if (err != null) *err = Native.Capture(new InvalidOperationException("pwrs: cmdlet stream and session-state calls are only valid on the pipeline thread"));
                return true;
            }

            private static object[] Args(IntPtr h)
            {
                object? o = Base(h);
                if (o == null) return Array.Empty<object>();
                if (o is object[] arr) return arr;
                if (o is Array a)
                {
                    var copy = new object[a.Length];
                    for (int i = 0; i < a.Length; i++) copy[i] = a.GetValue(i)!;
                    return copy;
                }
                return new[] { o };
            }

            private static int WriteChars(string s, ushort* buf, nuint cap, nuint* len)
            {
                if (len != null) *len = (nuint)s.Length;
                int n = (int)Math.Min((ulong)s.Length, (ulong)cap);
                if (buf != null && n > 0)
                {
                    fixed (char* src = s) Buffer.MemoryCopy(src, buf, (long)cap * 2, (long)n * 2);
                }
                return Native.Ok;
            }

            // ---- handle lifecycle ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static void FreeHandle(IntPtr h)
            {
                if (h != IntPtr.Zero) GCHandle.FromIntPtr(h).Free();
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr CloneHandle(IntPtr h) => Alloc(Target(h));

            // ---- cmdlet streams ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteObject(IntPtr c, IntPtr obj, byte enumerate, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try { cmd.WriteObject(Target(obj), enumerate != 0); return Native.Ok; }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteError(IntPtr c, PsStr16 message, PsStr16 id, uint category, IntPtr target, byte terminating, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try
                {
                    Raise(cmd, new PwrsException(message.ToString()), id, category, target, terminating);
                    return Native.Ok;
                }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

            /// <summary>
            /// WriteError for an error with details: the record's exception
            /// has the message, and its InnerException has the
            /// details, so the default view is still the one line.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteErrorDetails(IntPtr c, PsStr16 message, PsStr16 id, uint category, IntPtr target, byte terminating, PsStr16 details, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try
                {
                    Raise(cmd, new PwrsException(message.ToString(), new PwrsException(details.ToString())), id, category, target, terminating);
                    return Native.Ok;
                }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

            /// <summary>
            /// WriteError for an error that names its exception type: the
            /// record's exception is the one <paramref name="exception"/>
            /// holds, which <see cref="ExceptionNamed"/> made, so a
            /// script's catch of that type catches the record.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteErrorException(IntPtr c, IntPtr exception, PsStr16 id, uint category, IntPtr target, byte terminating, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try
                {
                    Exception named = ExceptionOf(exception);
                    NamedException.SetOwnRecord(named, id.ToString(), (ErrorCategory)category, Target(target));
                    Raise(cmd, named, id, category, target, terminating);
                    return Native.Ok;
                }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

            /// <summary>
            /// The exception a handle from <see cref="ExceptionNamed"/>
            /// holds, or one saying the handle holds none.
            /// </summary>
            private static Exception ExceptionOf(IntPtr handle) =>
                Target(handle) as Exception ?? new PwrsException("an error was given an exception handle that holds no exception");

            /// <summary>
            /// Writes the record <paramref name="exception"/> makes, or stores
            /// it for ThrowTerminatingError when <paramref name="terminating"/>
            /// is set.
            /// </summary>
            private static void Raise(RustCmdlet cmd, Exception exception, PsStr16 id, uint category, IntPtr target, byte terminating)
            {
                var record = new ErrorRecord(exception, id.ToString(), (ErrorCategory)category, Target(target));
                if (terminating != 0) cmd.SetPendingTerminating(record);
                else cmd.WriteError(record);
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteStream(IntPtr c, uint kind, PsStr16 text, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try
                {
                    string s = text.ToString();
                    switch (kind)
                    {
                        case Native.StreamVerbose: cmd.WriteVerbose(s); break;
                        case Native.StreamDebug: cmd.WriteDebug(s); break;
                        case Native.StreamWarning: cmd.WriteWarning(s); break;
                        case Native.StreamInformation: cmd.WriteInformation(s, null); break;
                        default: return Fail(new ArgumentOutOfRangeException(nameof(kind), $"unknown stream kind {kind}"), err);
                    }
                    return Native.Ok;
                }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteProgress(IntPtr c, int id, PsStr16 activity, PsStr16 status, int percent, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try
                {
                    var rec = new ProgressRecord(id, activity.ToString(), status.ToString());
                    if (percent < 0) rec.RecordType = ProgressRecordType.Completed;
                    else rec.PercentComplete = Math.Min(percent, 100);
                    cmd.WriteProgress(rec);
                    return Native.Ok;
                }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

            /// <summary>
            /// WriteProgress of a record with every field set: completed
            /// when <paramref name="completed"/> is nonzero and processing
            /// otherwise, so a processing record can show no percentage.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteProgressRecord(IntPtr c, int id, int parent, PsStr16 activity, PsStr16 status, PsStr16 operation, int percent, int seconds, byte completed, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try
                {
                    var rec = new ProgressRecord(id, activity.ToString(), status.ToString());
                    if (parent >= 0) rec.ParentActivityId = parent;
                    if (operation.Len != 0) rec.CurrentOperation = operation.ToString();
                    rec.PercentComplete = Math.Min(percent, 100);
                    rec.SecondsRemaining = seconds;
                    if (completed != 0) rec.RecordType = ProgressRecordType.Completed;
                    cmd.WriteProgress(rec);
                    return Native.Ok;
                }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ShouldProcess(IntPtr c, PsStr16 target, PsStr16 action, byte* yes, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try { *yes = (byte)(cmd.ShouldProcess(target.ToString(), action.ToString()) ? 1 : 0); return Native.Ok; }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ShouldContinue(IntPtr c, PsStr16 query, PsStr16 caption, byte* yes, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try { *yes = (byte)(cmd.ShouldContinue(query.ToString(), caption.ToString()) ? 1 : 0); return Native.Ok; }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ShouldProcessForm(IntPtr c, byte form, PsStr16 a, PsStr16 b, PsStr16 caption, byte* yes, byte* reason, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try
                {
                    bool answer;
                    switch (form)
                    {
                        case 0:
                            answer = cmd.ShouldProcess(a.ToString());
                            break;
                        case 1:
                            answer = cmd.ShouldProcess(a.ToString(), b.ToString(), caption.ToString());
                            break;
                        default:
                            answer = cmd.ShouldProcess(a.ToString(), b.ToString(), caption.ToString(), out ShouldProcessReason why);
                            *reason = (byte)(why == ShouldProcessReason.WhatIf ? 1 : 0);
                            break;
                    }
                    *yes = (byte)(answer ? 1 : 0);
                    return Native.Ok;
                }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ShouldContinueAll(IntPtr c, PsStr16 query, PsStr16 caption, byte impact, byte* yes, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try
                {
                    bool answer = impact == 0
                        ? cmd.ShouldContinue(query.ToString(), caption.ToString(), ref cmd.YesToAll, ref cmd.NoToAll)
                        : cmd.ShouldContinue(query.ToString(), caption.ToString(), impact == 2, ref cmd.YesToAll, ref cmd.NoToAll);
                    *yes = (byte)(answer ? 1 : 0);
                    return Native.Ok;
                }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

            // ---- paging ----

            /// <summary>
            /// The refusal for a paging call from a cmdlet whose
            /// <c>[Cmdlet]</c> does not declare <c>SupportsPaging</c>, for
            /// which the engine gives no <c>PagingParameters</c>.
            /// </summary>
            private static InvalidOperationException NoPaging(RustCmdlet cmd) =>
                new InvalidOperationException($"{cmd.MyInvocation.MyCommand.Name} does not declare supports_paging, so it has no -First, -Skip or -IncludeTotalCount to read, and no total count to write");

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int Paging(IntPtr c, ulong* first, ulong* skip, byte* includeTotalCount, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try
                {
                    PagingParameters? paging = cmd.PagingParameters;
                    if (paging == null) return Fail(NoPaging(cmd), err);
                    *first = paging.First;
                    *skip = paging.Skip;
                    *includeTotalCount = (byte)(paging.IncludeTotalCount.IsPresent ? 1 : 0);
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteTotalCount(IntPtr c, ulong total, double accuracy, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                PagingParameters? paging = cmd.PagingParameters;
                if (paging == null) return Fail(NoPaging(cmd), err);
                try { cmd.WriteObject(paging.NewTotalCount(total, accuracy)); return Native.Ok; }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

            // ---- jobs ----

            private static RustJob Job(IntPtr job) =>
                Target(job) as RustJob ?? throw new PwrsException("the handle given as a job is not a RustJob");

            /// <summary>
            /// A RustJob for the cmdlet, registered through its JobManager so
            /// the job cmdlets of its runspace find it, and Running. On a
            /// throw the native side still owns the context.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int JobNew(IntPtr c, PsStr16 name, IntPtr stop, IntPtr context, IntPtr release, IntPtr* @out, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try
                {
                    Type adapter = cmd.OwningModule.JobSourceAdapter
                        ?? throw new PwrsException("this module's shell declares no job source adapter; build it again with this cargo-pwrs");
                    string command = cmd.MyInvocation.MyCommand.Name;
                    var job = new RustJob(command, name.Len == 0 ? null : name.ToString(), stop, context, release);
                    var parameters = new System.Collections.Generic.Dictionary<string, object> { { RustJobSourceAdapter.NewJobKey, job } };
                    cmd.JobManager.NewJob(new JobInvocationInfo(new JobDefinition(adapter, command, job.Name), parameters));
                    job.Begin();
                    *@out = Alloc(job);
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int JobWrite(IntPtr job, IntPtr value, byte enumerate, IntPtr* err)
            {
                try
                {
                    RustJob j = Job(job);
                    object? v = Target(value);
                    if (enumerate != 0 && LanguagePrimitives.GetEnumerable(v) is System.Collections.IEnumerable items)
                    {
                        foreach (object? item in items) j.Output.Add(item == null ? null : PSObject.AsPSObject(item));
                    }
                    else
                    {
                        j.Output.Add(v == null ? null : PSObject.AsPSObject(v));
                    }
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int JobWriteStream(IntPtr job, uint kind, PsStr16 text, IntPtr* err)
            {
                try
                {
                    RustJob j = Job(job);
                    string s = text.ToString();
                    switch (kind)
                    {
                        case Native.StreamVerbose: j.Verbose.Add(new VerboseRecord(s)); break;
                        case Native.StreamDebug: j.Debug.Add(new DebugRecord(s)); break;
                        case Native.StreamWarning: j.Warning.Add(new WarningRecord(s)); break;
                        case Native.StreamInformation: j.Information.Add(new InformationRecord(s, j.Command)); break;
                        default: return Fail(new ArgumentOutOfRangeException(nameof(kind), $"unknown stream kind {kind}"), err);
                    }
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int JobWriteError(IntPtr job, PsStr16 message, PsStr16 id, uint category, IntPtr target, PsStr16 details, IntPtr* err)
            {
                try
                {
                    RustJob j = Job(job);
                    Exception exception = details.Len == 0
                        ? new PwrsException(message.ToString())
                        : new PwrsException(message.ToString(), new PwrsException(details.ToString()));
                    j.Error.Add(new ErrorRecord(exception, id.ToString(), (ErrorCategory)category, Target(target)));
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// Adds to the job's error stream a record whose exception is the
            /// one <paramref name="exception"/> holds, which
            /// <see cref="ExceptionNamed"/> made.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int JobWriteErrorException(IntPtr job, IntPtr exception, PsStr16 id, uint category, IntPtr target, IntPtr* err)
            {
                try
                {
                    Exception named = ExceptionOf(exception);
                    NamedException.SetOwnRecord(named, id.ToString(), (ErrorCategory)category, Target(target));
                    Job(job).Error.Add(new ErrorRecord(named, id.ToString(), (ErrorCategory)category, Target(target)));
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int JobWriteProgress(IntPtr job, int id, int parent, PsStr16 activity, PsStr16 status, PsStr16 operation, int percent, int seconds, byte completed, IntPtr* err)
            {
                try
                {
                    RustJob j = Job(job);
                    var rec = new ProgressRecord(id, activity.ToString(), status.ToString());
                    if (parent >= 0) rec.ParentActivityId = parent;
                    if (operation.Len != 0) rec.CurrentOperation = operation.ToString();
                    rec.PercentComplete = Math.Min(percent, 100);
                    rec.SecondsRemaining = seconds;
                    if (completed != 0) rec.RecordType = ProgressRecordType.Completed;
                    j.Progress.Add(rec);
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int JobFinish(IntPtr job, byte failed, PsStr16 message, PsStr16 details, IntPtr* err)
            {
                try
                {
                    Job(job).Finish(failed != 0, message.ToString(), details.ToString());
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// Ends the job as JobFinish does for a failure, its reason the
            /// exception <paramref name="exception"/> holds, which
            /// <see cref="ExceptionNamed"/> made.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int JobFinishException(IntPtr job, IntPtr exception, IntPtr* err)
            {
                try
                {
                    Job(job).Finish(ExceptionOf(exception));
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            // ---- transactions ----

            /// <summary>
            /// transaction_current: the System.Transactions.Transaction of the
            /// PowerShell transaction the cmdlet runs in, or null when
            /// TransactionAvailable() is false. A cmdlet whose [Cmdlet] does
            /// not declare SupportsTransactions is refused, by name.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int TransactionCurrent(IntPtr c, IntPtr* @out, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try
                {
                    if (!Transactions.Declared(cmd.GetType()))
                    {
                        return Fail(new InvalidOperationException($"{cmd.MyInvocation.MyCommand.Name} does not declare supports_transactions, so it has no -UseTransaction and no transaction to read"), err);
                    }
                    *@out = Alloc(Transactions.Current(cmd));
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// transaction_enlist: enlists the native callbacks in the
            /// transaction the handle <paramref name="transaction"/> refers to, as a
            /// <see cref="RustEnlistment"/>. On success the enlistment owns
            /// the context; on a throw, such as for a transaction that has
            /// ended, the caller still does.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int TransactionEnlist(IntPtr transaction, IntPtr prepare, IntPtr end, IntPtr context, IntPtr* err)
            {
                try
                {
                    var t = Base(transaction) as System.Transactions.Transaction
                        ?? throw new ArgumentException("pwrs: transaction_enlist was given something other than a transaction");
                    t.EnlistVolatile(new RustEnlistment(prepare, end, context), System.Transactions.EnlistmentOptions.None);
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            // ---- steppable pipelines ----

            /// <summary>
            /// The common parameters steppable_begin passes on from the
            /// wrapper to the command it begins: the ones setting a
            /// preference for the command, not the ones naming a variable,
            /// which the wrapper and the command would both assign.
            /// </summary>
            private static readonly string[] SteppableForwarded = { "WhatIf", "Confirm", "Verbose", "Debug", "ErrorAction", "WarningAction", "InformationAction" };

            /// <summary>
            /// steppable_begin: begins the command named
            /// <paramref name="name"/>, with the hashtable
            /// in <paramref name="parameters"/> splatted, as a
            /// SteppablePipeline whose output and streams are the cmdlet's.
            /// The name reaches the script block as a string, so the engine
            /// resolves it as a typed command and a module's function runs
            /// in its module's scope. A forwarded common parameter the
            /// caller gave the cmdlet is added unless the hashtable names it
            /// or the command does not take it. <paramref name="delegates"/>
            /// is 1 when the command takes -WhatIf, so the cmdlet's
            /// SupportsShouldProcess is the command's to honor.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int SteppableBegin(IntPtr c, PsStr16 name, IntPtr parameters, IntPtr* @out, byte* delegates, IntPtr* err)
            {
                *delegates = 0;
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try
                {
                    string command = name.ToString();
                    var splat = new Hashtable(StringComparer.OrdinalIgnoreCase);
                    if (parameters != IntPtr.Zero && Base(parameters) is IDictionary given)
                    {
                        foreach (DictionaryEntry entry in given) splat[entry.Key] = entry.Value;
                    }
                    System.Collections.Generic.IDictionary<string, ParameterMetadata>? takes = TakenParameters(cmd, command);
                    foreach (string key in SteppableForwarded)
                    {
                        if (splat.ContainsKey(key) || takes == null || !takes.ContainsKey(key)) continue;
                        if (cmd.MyInvocation.BoundParameters.TryGetValue(key, out object? value)) splat[key] = value;
                    }
                    ScriptBlock script = ScriptBlock.Create("param($c, $a) & $c @a");
                    SteppablePipeline steppable = script.GetSteppablePipeline(cmd.MyInvocation.CommandOrigin, new object[] { command, splat });
                    steppable.Begin(cmd);
                    *@out = Alloc(steppable);
                    *delegates = (byte)(takes != null && takes.ContainsKey("WhatIf") ? 1 : 0);
                    return Native.Ok;
                }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

            /// <summary>
            /// The parameters the command named <paramref name="command"/>
            /// takes, an alias followed to what it names, or null for one
            /// with no parameters of its own to read, such as an
            /// application, and for a name no command has, which
            /// GetSteppablePipeline then reports as the engine does.
            /// </summary>
            private static System.Collections.Generic.IDictionary<string, ParameterMetadata>? TakenParameters(RustCmdlet cmd, string command)
            {
                CommandInfo? info = cmd.InvokeCommand.GetCommand(command, CommandTypes.All);
                while (info is AliasInfo alias && alias.ResolvedCommand != null) info = alias.ResolvedCommand;
                return info is CmdletInfo || info is FunctionInfo || info is ExternalScriptInfo ? info.Parameters : null;
            }

            private static SteppablePipeline Steppable(IntPtr h) =>
                Base(h) as SteppablePipeline ?? throw new ArgumentException("pwrs: a steppable entry was given something other than a SteppablePipeline");

            /// <summary>
            /// steppable_process: Process(input), or Process() when
            /// <paramref name="hasInput"/> is 0. The input goes as the
            /// handle's object, so a PSObject does not lose its note
            /// properties.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int SteppableProcess(IntPtr c, IntPtr steppable, IntPtr input, byte hasInput, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try
                {
                    SteppablePipeline p = Steppable(steppable);
                    if (hasInput != 0) p.Process(Target(input));
                    else p.Process();
                    return Native.Ok;
                }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

            /// <summary>steppable_end: End() on the pipeline.</summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int SteppableEnd(IntPtr c, IntPtr steppable, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try
                {
                    Steppable(steppable).End();
                    return Native.Ok;
                }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

            /// <summary>
            /// steppable_dispose: Clean() where the engine has it, so the
            /// clean blocks of the commands begun run, then Dispose().
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int SteppableDispose(IntPtr c, IntPtr steppable, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try
                {
                    SteppablePipeline p = Steppable(steppable);
#if NET
                    p.Clean();
#endif
                    p.Dispose();
                    return Native.Ok;
                }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

            // ---- wrapped commands ----

            /// <summary>
            /// The common parameters wrapped_begin passes on from the
            /// wrapping cmdlet to its command: the preference ones, as
            /// steppable_begin passes, and those naming a variable for the
            /// error, warning and information records, which the command
            /// writes straight to the caller's streams and so is the one to
            /// record, as the proxy function ProxyCommand.Create writes
            /// passes them on.
            /// </summary>
            private static readonly string[] WrappedForwarded =
                { "WhatIf", "Confirm", "Verbose", "Debug", "ErrorAction", "WarningAction", "InformationAction", "ErrorVariable", "WarningVariable", "InformationVariable" };

            private static WrappedCmdlet Wrapping(RustCmdlet c) =>
                c as WrappedCmdlet ?? throw new InvalidOperationException($"pwrs: {c.MyInvocation.MyCommand.Name} wraps no command");

            /// <summary>
            /// Whether <paramref name="e"/> is, or contains, the abort a
            /// wrapped command's output hook throws: the native side stores
            /// the hook's error and reports it itself, so the abort is not
            /// stored as an exception passing through.
            /// </summary>
            private static bool HookFailed(Exception e)
            {
                for (Exception? x = e; x != null; x = x.InnerException)
                {
                    if (x is WrappedHookFailed) return true;
                }
                return false;
            }

            /// <summary>
            /// wrapped_arguments: the arguments the caller bound to the
            /// parameters copied from the wrapped command, as a new table.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WrappedArguments(IntPtr c, IntPtr* @out, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try
                {
                    *@out = Alloc(Wrapping(cmd).Arguments());
                    return Native.Ok;
                }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

            /// <summary>
            /// wrapped_input: the record the pipeline passes to the cmdlet now,
            /// as it was piped, or none when it takes no pipeline input.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WrappedInput(IntPtr c, IntPtr* @out, byte* has, IntPtr* err)
            {
                *has = 0;
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try
                {
                    if (Wrapping(cmd).TryInput(out object? record))
                    {
                        *@out = Alloc(record);
                        *has = 1;
                    }
                    return Native.Ok;
                }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

            /// <summary>
            /// wrapped_begin: begins the wrapped command with the arguments
            /// splatted and the caller's preference common parameters added
            /// as steppable_begin adds them, its output passing through the
            /// hook. <paramref name="delegates"/> is as steppable_begin's.
            /// A cmdlet that does not run the command in this PowerShell is
            /// refused with its reason.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WrappedBegin(IntPtr c, IntPtr arguments, IntPtr hook, IntPtr context, byte* delegates, IntPtr* err)
            {
                *delegates = 0;
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                WrappedCmdlet wrapping;
                try
                {
                    wrapping = Wrapping(cmd);
                    wrapping.CheckRefusal();
                }
                catch (Exception e) { return Fail(e, err); }
                try
                {
                    var splat = new Hashtable(StringComparer.OrdinalIgnoreCase);
                    if (arguments != IntPtr.Zero && Base(arguments) is IDictionary given)
                    {
                        foreach (DictionaryEntry entry in given) splat[entry.Key] = entry.Value;
                    }
                    System.Collections.Generic.IDictionary<string, ParameterMetadata>? takes = TakenParameters(cmd, wrapping.Name);
                    foreach (string key in WrappedForwarded)
                    {
                        if (splat.ContainsKey(key) || takes == null || !takes.ContainsKey(key)) continue;
                        if (cmd.MyInvocation.BoundParameters.TryGetValue(key, out object? value)) splat[key] = value;
                    }
                    wrapping.Begin(splat, hook, context);
                    *delegates = (byte)(takes != null && takes.ContainsKey("WhatIf") ? 1 : 0);
                    return Native.Ok;
                }
                catch (Exception e) when (HookFailed(e)) { return Native.ErrManagedException; }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

            /// <summary>
            /// wrapped_process: Process(input), or Process() when
            /// <paramref name="hasInput"/> is 0, on the wrapped command, its
            /// output passing through the hook.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WrappedProcess(IntPtr c, IntPtr input, byte hasInput, IntPtr hook, IntPtr context, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try
                {
                    Wrapping(cmd).Process(Target(input), hasInput != 0, hook, context);
                    return Native.Ok;
                }
                catch (Exception e) when (HookFailed(e)) { return Native.ErrManagedException; }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

            /// <summary>
            /// wrapped_end: End() on the wrapped command, its output passing
            /// through the hook, then its clean blocks and disposal.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WrappedEnd(IntPtr c, IntPtr hook, IntPtr context, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try
                {
                    Wrapping(cmd).End(hook, context);
                    return Native.Ok;
                }
                catch (Exception e) when (HookFailed(e)) { return Native.ErrManagedException; }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

            // ---- parameters ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int GetParameter(IntPtr c, PsStr16 name, IntPtr* @out, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try
                {
                    cmd.MyInvocation.BoundParameters.TryGetValue(name.ToString(), out object? v);
                    *@out = Alloc(v);
                    return Native.Ok;
                }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ParameterIsBound(IntPtr c, PsStr16 name, byte* bound)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, null)) return Native.ErrWrongThread;
                try
                {
                    *bound = (byte)(cmd.MyInvocation.BoundParameters.ContainsKey(name.ToString()) ? 1 : 0);
                    return Native.Ok;
                }
                catch (Exception e) { return FailOn(cmd, e, null); }
            }

            // ---- primitives ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr StringNew(PsStr16 s) => Alloc(s.ToString());

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int StringRead(IntPtr h, ushort* buf, nuint cap, nuint* len, IntPtr* err)
            {
                try
                {
                    string s = LanguagePrimitives.ConvertTo<string>(Target(h)) ?? string.Empty;
                    return WriteChars(s, buf, cap, len);
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr I64New(long v) => Alloc(v);

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int I64Read(IntPtr h, long* v, IntPtr* err)
            {
                try { *v = LanguagePrimitives.ConvertTo<long>(Target(h)); return Native.Ok; }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr F64New(double v) => Alloc(v);

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int F64Read(IntPtr h, double* v, IntPtr* err)
            {
                try { *v = LanguagePrimitives.ConvertTo<double>(Target(h)); return Native.Ok; }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr BoolNew(byte v) => Alloc(v != 0);

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int BoolRead(IntPtr h, byte* v, IntPtr* err)
            {
                try { *v = (byte)(LanguagePrimitives.ConvertTo<bool>(Target(h)) ? 1 : 0); return Native.Ok; }
                catch (Exception e) { return Fail(e, err); }
            }

            // ---- arrays ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ArrayLen(IntPtr h, nuint* len, IntPtr* err)
            {
                try
                {
                    object? o = Base(h);
                    switch (o)
                    {
                        case null: *len = 0; break;
                        case Array a: *len = (nuint)a.Length; break;
                        case ICollection col: *len = (nuint)col.Count; break;
                        case IEnumerable en:
                            {
                                nuint n = 0;
                                foreach (var _ in en) n++;
                                *len = n;
                                break;
                            }
                        default: throw new InvalidOperationException($"pwrs: {o.GetType().FullName} is not a collection");
                    }
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ArrayGet(IntPtr h, nuint i, IntPtr* @out, IntPtr* err)
            {
                try
                {
                    object? o = Base(h);
                    int index = checked((int)i);
                    switch (o)
                    {
                        case Array a: *@out = Alloc(a.GetValue(index)); break;
                        case IList l: *@out = Alloc(l[index]); break;
                        case IEnumerable en:
                            {
                                int k = 0;
                                foreach (var item in en)
                                {
                                    if (k++ == index) { *@out = Alloc(item); return Native.Ok; }
                                }
                                throw new IndexOutOfRangeException();
                            }
                        default: throw new InvalidOperationException("pwrs: value is not a collection");
                    }
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ArrayNew(uint tag, nuint len, IntPtr* @out, IntPtr* err)
            {
                try { *@out = Alloc(Array.CreateInstance(Native.ElementType(tag), checked((int)len))); return Native.Ok; }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ArraySet(IntPtr h, nuint i, IntPtr value, IntPtr* err)
            {
                try
                {
                    object? o = Base(h);
                    int index = checked((int)i);
                    object? v = Target(value);
                    switch (o)
                    {
                        case Array a: a.SetValue(LanguagePrimitives.ConvertTo(v, a.GetType().GetElementType()!), index); break;
                        case IList l: l[index] = v; break;
                        default: throw new InvalidOperationException("pwrs: value is not a writable collection");
                    }
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ArrayPin(IntPtr h, PsPinned* @out, IntPtr* err)
            {
                try
                {
                    if (!(Base(h) is Array a)) throw new InvalidOperationException("pwrs: value is not an array");
                    Type elem = a.GetType().GetElementType()!;
                    // Decimal is blittable and pins, but Type.IsPrimitive
                    // is false for it, so it is admitted by name.
                    if (!elem.IsPrimitive && elem != typeof(decimal))
                        throw new InvalidOperationException($"pwrs: cannot pin an array of {elem.FullName}");
                    var pin = GCHandle.Alloc(a, GCHandleType.Pinned);
                    @out->Data = (void*)pin.AddrOfPinnedObject();
                    @out->Len = (nuint)a.Length;
                    @out->ElemSize = (uint)Marshal.SizeOf(elem);
                    @out->Pin = GCHandle.ToIntPtr(pin);
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static void ArrayUnpin(PsPinned p)
            {
                if (p.Pin != IntPtr.Zero) GCHandle.FromIntPtr(p.Pin).Free();
            }

            /// <summary>
            /// The string StringRead would copy, pinned where it lies, so
            /// the native side reads its characters with no copy. A
            /// pinned string's address is that of its first character,
            /// on .NET and on .NET Framework alike. ArrayUnpin frees it.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int StringPin(IntPtr h, PsPinned* @out, IntPtr* err)
            {
                try
                {
                    string s = LanguagePrimitives.ConvertTo<string>(Target(h)) ?? string.Empty;
                    var pin = GCHandle.Alloc(s, GCHandleType.Pinned);
                    @out->Data = (void*)pin.AddrOfPinnedObject();
                    @out->Len = (nuint)s.Length;
                    @out->ElemSize = sizeof(char);
                    @out->Pin = GCHandle.ToIntPtr(pin);
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            // ---- PSObject ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr PsObjectNew(PsStr16 typeName)
            {
                var ps = new PSObject();
                string name = typeName.ToString();
                if (name.Length > 0) ps.TypeNames.Insert(0, name);
                return Alloc(ps);
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int PsObjectAddNote(IntPtr obj, PsStr16 name, IntPtr value, IntPtr* err)
            {
                try
                {
                    var ps = PSObject.AsPSObject(Target(obj));
                    ps.Properties.Add(new PSNoteProperty(name.ToString(), Target(value)));
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int PsObjectGetProperty(IntPtr obj, PsStr16 name, IntPtr* @out, IntPtr* err)
            {
                try
                {
                    var ps = PSObject.AsPSObject(Target(obj));
                    var prop = ps.Properties[name.ToString()];
                    if (prop == null) throw new ArgumentException($"pwrs: no property named {name}");
                    *@out = Alloc(prop.Value);
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            // ---- session state ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int GetVariable(IntPtr c, PsStr16 name, IntPtr* @out, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try { *@out = Alloc(cmd.SessionState.PSVariable.GetValue(name.ToString())); return Native.Ok; }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int SetVariable(IntPtr c, PsStr16 name, IntPtr value, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try { cmd.SessionState.PSVariable.Set(name.ToString(), Target(value)); return Native.Ok; }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ResolvePath(IntPtr c, PsStr16 path, byte literal, IntPtr* @out, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try
                {
                    string p = path.ToString();
                    if (literal != 0)
                    {
                        *@out = Alloc(new[] { cmd.SessionState.Path.GetUnresolvedProviderPathFromPSPath(p) });
                    }
                    else
                    {
                        Collection<string> paths = cmd.SessionState.Path.GetResolvedProviderPathFromPSPath(p, out ProviderInfo _);
                        var arr = new string[paths.Count];
                        paths.CopyTo(arr, 0);
                        *@out = Alloc(arr);
                    }
                    return Native.Ok;
                }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int InvokeScriptBlock(IntPtr c, IntPtr block, IntPtr args, IntPtr* @out, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try
                {
                    if (!(Base(block) is ScriptBlock sb)) throw new ArgumentException("pwrs: value is not a ScriptBlock");
                    Collection<PSObject> results = sb.Invoke(Args(args));
                    var arr = new object[results.Count];
                    for (int i = 0; i < arr.Length; i++) arr[i] = results[i];
                    *@out = Alloc(arr);
                    return Native.Ok;
                }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int InvokeCommand(IntPtr c, PsStr16 name, IntPtr parameters, IntPtr input, IntPtr* @out, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try
                {
                    using (var ps = PowerShell.Create(RunspaceMode.CurrentRunspace))
                    {
                        // Added by name, which the engine resolves as it does
                        // a typed command: a module's function runs in its
                        // module's scope, so a dynamicparam block reading the
                        // module's $script: state works, as it does not when
                        // the function is added as a CommandInfo. A name is
                        // not script text, so nothing is parsed, and a name
                        // that matches no command is PowerShell's own
                        // CommandNotFoundException.
                        ps.AddCommand(name.ToString());
                        if (parameters != IntPtr.Zero && Base(parameters) is IDictionary dict) ps.AddParameters(dict);
                        object? piped = input == IntPtr.Zero ? null : Base(input);
                        Collection<PSObject> results = piped switch
                        {
                            null => ps.Invoke(),
                            // A string enumerates its characters; it is one record.
                            string s => ps.Invoke(new object[] { s }),
                            IEnumerable items => ps.Invoke(items),
                            object one => ps.Invoke(new object[] { one }),
                        };
                        // The command's non-terminating errors reach the
                        // caller's error stream, as they would had the user
                        // run it, and its output is collected.
                        foreach (ErrorRecord record in ps.Streams.Error) cmd.WriteError(record);
                        var arr = new object[results.Count];
                        for (int i = 0; i < arr.Length; i++) arr[i] = results[i];
                        *@out = Alloc(arr);
                    }
                    return Native.Ok;
                }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

            // ---- a value of a CLR enum the module did not declare ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int EnumNew(PsStr16 typeName, long value, IntPtr* @out, IntPtr* err)
            {
                try
                {
                    // Resolved the way a type literal is, so a name the
                    // engine cannot reach is the error rather than a
                    // value of the wrong type.
                    Type t = LanguagePrimitives.ConvertTo<Type>(typeName.ToString());
                    if (!t.IsEnum) throw new ArgumentException($"pwrs: {t.FullName} is not an enum");
                    *@out = Alloc(Enum.ToObject(t, value));
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            // ---- dynamic .NET access ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int DynGet(IntPtr obj, PsStr16 name, IntPtr* @out, IntPtr* err)
            {
                try
                {
                    object? target = Target(obj);
                    string key = name.ToString();
                    var ps = PSObject.AsPSObject(target);
                    var member = ps.Members[key];
                    if (member != null)
                    {
                        *@out = Alloc(member.Value);
                        return Native.Ok;
                    }
                    if (ps.BaseObject is IDictionary dict && dict.Contains(key))
                    {
                        *@out = Alloc(dict[key]);
                        return Native.Ok;
                    }
                    throw new ArgumentException($"pwrs: no member named {key}");
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int DynSet(IntPtr obj, PsStr16 name, IntPtr value, IntPtr* err)
            {
                try
                {
                    var ps = PSObject.AsPSObject(Target(obj));
                    var prop = ps.Properties[name.ToString()];
                    if (prop == null) throw new ArgumentException($"pwrs: no property named {name}");
                    prop.Value = Target(value);
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// On a table, get_Item, set_Item, ContainsKey, Contains and
            /// Remove are called through the IDictionary it is, so a
            /// PsHashtable read needs neither the adapter's overload
            /// resolution nor the reflection that get_Item, which the
            /// adapter hides, otherwise takes; a missing key reads as null
            /// whatever the table's own indexer does. Everything else goes
            /// to PowerShell's member binder first and the CLR binder
            /// second, which is what reaches the accessors the adapter
            /// hides.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int DynCall(IntPtr obj, PsStr16 name, IntPtr args, IntPtr* @out, IntPtr* err)
            {
                try
                {
                    object? target = Target(obj);
                    string key = name.ToString();
                    object[] a = Args(args);
                    if ((target is PSObject wrappedTarget ? wrappedTarget.BaseObject : target) is IDictionary table)
                    {
                        switch (key)
                        {
                            case "get_Item" when a.Length == 1:
                                *@out = Alloc(table[a[0]]);
                                return Native.Ok;
                            case "set_Item" when a.Length == 2:
                                table[a[0]] = a[1];
                                *@out = IntPtr.Zero;
                                return Native.Ok;
                            case "ContainsKey" when a.Length == 1:
                            case "Contains" when a.Length == 1:
                                *@out = Alloc(table.Contains(a[0]));
                                return Native.Ok;
                            case "Remove" when a.Length == 1:
                                table.Remove(a[0]);
                                *@out = IntPtr.Zero;
                                return Native.Ok;
                        }
                    }
                    var ps = PSObject.AsPSObject(target);
                    var method = ps.Methods[key];
                    if (method != null)
                    {
                        *@out = Alloc(method.Invoke(a));
                        return Native.Ok;
                    }
                    object? baseObj = ps.BaseObject;
                    if (baseObj == null) throw new ArgumentException($"pwrs: no method named {key}");
                    object? r = baseObj.GetType().InvokeMember(key, BindingFlags.Public | BindingFlags.Instance | BindingFlags.InvokeMethod, null, baseObj, a);
                    *@out = Alloc(r);
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int DynCallStatic(PsStr16 typeName, PsStr16 name, IntPtr args, IntPtr* @out, IntPtr* err)
            {
                try
                {
                    Type t = LanguagePrimitives.ConvertTo<Type>(typeName.ToString());
                    *@out = Alloc(Bind(t, name.ToString(), Args(args)));
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int DynNew(PsStr16 typeName, IntPtr args, IntPtr* @out, IntPtr* err)
            {
                try
                {
                    Type t = LanguagePrimitives.ConvertTo<Type>(typeName.ToString());
                    *@out = Alloc(Bind(t, null, Args(args)));
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// Calls the public static method of <paramref name="t"/> named
            /// <paramref name="method"/>, or with a null name its public
            /// constructor, that fits <paramref name="args"/>. .NET's own
            /// binding comes first, so arguments that already have the
            /// parameters' types bind as .NET binds them. When no overload
            /// fits that way, each overload with as many parameters is tried,
            /// every argument converted as PowerShell converts a value to the
            /// parameter's type, and the one needing the fewest conversions is
            /// called, the first declared among equals. A method's name
            /// matches without regard to case, as in a script.
            /// </summary>
            private static object? Bind(Type t, string? method, object[] args)
            {
                try
                {
                    return method == null
                        ? Activator.CreateInstance(t, args)
                        : t.InvokeMember(method, BindingFlags.Public | BindingFlags.Static | BindingFlags.InvokeMethod, null, null, args);
                }
                catch (MissingMethodException)
                {
                    MethodBase[] candidates = method == null
                        ? t.GetConstructors()
                        : Array.FindAll(t.GetMethods(BindingFlags.Public | BindingFlags.Static), m => string.Equals(m.Name, method, StringComparison.OrdinalIgnoreCase));
                    Array.Sort(candidates, (a, b) => a.MetadataToken.CompareTo(b.MetadataToken));
                    MethodBase? best = null;
                    object?[]? bestArgs = null;
                    int fewest = int.MaxValue;
                    foreach (MethodBase candidate in candidates)
                    {
                        object?[]? converted = Converted(candidate, args, out int conversions);
                        if (converted != null && conversions < fewest)
                        {
                            best = candidate;
                            bestArgs = converted;
                            fewest = conversions;
                        }
                    }
                    if (best == null) throw;
                    return best is ConstructorInfo constructor ? constructor.Invoke(bestArgs) : best.Invoke(null, bestArgs);
                }
            }

            /// <summary>
            /// <paramref name="args"/> as <paramref name="candidate"/>'s
            /// parameters take them, each converted as PowerShell converts a
            /// value where its type does not already fit, with the count of
            /// those converted; null when the counts differ, a parameter is
            /// passed by reference, or an argument does not convert.
            /// </summary>
            private static object?[]? Converted(MethodBase candidate, object[] args, out int conversions)
            {
                conversions = 0;
                ParameterInfo[] parameters = candidate.GetParameters();
                if (parameters.Length != args.Length || candidate.ContainsGenericParameters) return null;
                var converted = new object?[args.Length];
                for (int i = 0; i < args.Length; i++)
                {
                    Type wanted = parameters[i].ParameterType;
                    if (wanted.IsByRef) return null;
                    object? arg = args[i] is PSObject wrapped ? wrapped.BaseObject : args[i];
                    bool fits = arg == null ? !wanted.IsValueType || Nullable.GetUnderlyingType(wanted) != null : wanted.IsInstanceOfType(arg);
                    if (fits)
                    {
                        converted[i] = arg;
                    }
                    else if (LanguagePrimitives.TryConvertTo(args[i], wanted, out object? value))
                    {
                        converted[i] = value;
                        conversions++;
                    }
                    else
                    {
                        return null;
                    }
                }
                return converted;
            }

            // ---- generated types ----

            /// <summary>
            /// factory_new in the shared table. On .NET it uses the one
            /// table of the shell this copy of the assembly belongs to. On
            /// .NET Framework each module's native library receives a
            /// <c>ModuleTable</c> instead, so a call reaching this entry
            /// came through a table that belongs to no module, and is
            /// refused rather than run against a guessed module.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int FactoryNew(uint classId, void* fields, IntPtr* @out, IntPtr* err)
            {
#if NET
                return FactoryNewIn(Pwrs.Factories.OfThisCopy, classId, fields, @out, err);
#else
                return Fail(new InvalidOperationException($"pwrs: factory_new for class id {classId} reached the shared host table, which no module is given on .NET Framework"), err);
#endif
            }

            /// <summary>factory_new using one module's factories.</summary>
            internal static int FactoryNewIn(Factories factories, uint classId, void* fields, IntPtr* @out, IntPtr* err)
            {
                try { *@out = Alloc(factories.Create(classId, (IntPtr)fields)); return Native.Ok; }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// proxy_enter in the shared table, which on .NET Framework no
            /// module receives; see <see cref="FactoryNew"/>.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ProxyEnter(IntPtr obj, uint classId, IntPtr* instance, IntPtr* err)
            {
#if NET
                return ProxyEnterIn(Pwrs.Factories.OfThisCopy, obj, classId, instance, err, true);
#else
                return Fail(new InvalidOperationException($"pwrs: proxy_enter for class id {classId} reached the shared host table, which no module is given on .NET Framework"), err);
#endif
            }

            /// <summary>
            /// proxy_enter_shared in the shared table: the shared form of
            /// <see cref="ProxyEnter"/>, for a `with` borrow, which nests
            /// inside a `&self` method or a property read. On .NET
            /// Framework no module receives the shared table.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ProxyEnterShared(IntPtr obj, uint classId, IntPtr* instance, IntPtr* err)
            {
#if NET
                return ProxyEnterIn(Pwrs.Factories.OfThisCopy, obj, classId, instance, err, false);
#else
                return Fail(new InvalidOperationException($"pwrs: proxy_enter_shared for class id {classId} reached the shared host table, which no module is given on .NET Framework"), err);
#endif
            }

            /// <summary>
            /// Lends the value of a proxy object of class
            /// <paramref name="classId"/> of the module whose factories are
            /// <paramref name="factories"/>, exclusively or as shared;
            /// anything else is refused.
            /// </summary>
            internal static int ProxyEnterIn(Factories factories, IntPtr obj, uint classId, IntPtr* instance, IntPtr* err, bool exclusive)
            {
                try
                {
                    object? o = Base(obj);
                    if (!(o is IProxy p))
                    {
                        string got = o == null ? "$null" : "a " + o.GetType().FullName;
                        throw new PwrsException($"a proxy object of this module was expected, and {got} was passed");
                    }
                    *instance = p.PwrsCore.EnterBorrow(factories, classId, exclusive);
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>Ends a borrow <see cref="ProxyEnterIn"/> began.</summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static void ProxyExit(IntPtr obj, byte changed)
            {
                if (Base(obj) is IProxy p) p.PwrsCore.ExitBorrow(changed != 0);
            }

            // ---- helper executables ----

            /// <summary>
            /// helper_path in the shared table. On .NET it stages from the
            /// folder of the module this copy of the assembly belongs to; on
            /// .NET Framework it is refused, as <see cref="FactoryNew"/> is.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int HelperPath(PsStr16 name, ushort* buf, nuint cap, nuint* len, IntPtr* err)
            {
#if NET
                string? root = NativeModule.RootOfThisCopy;
                if (root == null)
                {
                    return Fail(new InvalidOperationException("pwrs: helper_path was called before the module's native library was loaded"), err);
                }
                return HelperPathIn(root, name, buf, cap, len, err);
#else
                return Fail(new InvalidOperationException($"pwrs: helper_path for {name} reached the shared host table, which no module is given on .NET Framework"), err);
#endif
            }

            /// <summary>helper_path staged from one module's folder.</summary>
            internal static int HelperPathIn(string moduleRoot, PsStr16 name, ushort* buf, nuint cap, nuint* len, IntPtr* err)
            {
                try { return WriteChars(NativeModule.StageHelper(moduleRoot, name.ToString()), buf, cap, len); }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// native_file_path in the shared table, staging from the folder
            /// helper_path stages from; on .NET Framework it is refused, as
            /// helper_path is.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int NativeFilePath(PsStr16 name, ushort* buf, nuint cap, nuint* len, IntPtr* err)
            {
#if NET
                string? root = NativeModule.RootOfThisCopy;
                if (root == null)
                {
                    return Fail(new InvalidOperationException("pwrs: native_file_path was called before the module's native library was loaded"), err);
                }
                return NativeFilePathIn(root, name, buf, cap, len, err);
#else
                return Fail(new InvalidOperationException($"pwrs: native_file_path for {name} reached the shared host table, which no module is given on .NET Framework"), err);
#endif
            }

            /// <summary>native_file_path staged from one module's folder.</summary>
            internal static int NativeFilePathIn(string moduleRoot, PsStr16 name, ushort* buf, nuint cap, nuint* len, IntPtr* err)
            {
                try { return WriteChars(NativeModule.StageFile(moduleRoot, name.ToString()), buf, cap, len); }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// library_folder in the shared table, staging from the folder
            /// helper_path stages from; on .NET Framework it is refused, as
            /// helper_path is.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int LibraryFolder(PsStr16 name, ushort* buf, nuint cap, nuint* len, IntPtr* err)
            {
#if NET
                string? root = NativeModule.RootOfThisCopy;
                if (root == null)
                {
                    return Fail(new InvalidOperationException("pwrs: library_folder was called before the module's native library was loaded"), err);
                }
                return LibraryFolderIn(root, name, buf, cap, len, err);
#else
                return Fail(new InvalidOperationException($"pwrs: library_folder for {name} reached the shared host table, which no module is given on .NET Framework"), err);
#endif
            }

            /// <summary>library_folder staged from one module's folder.</summary>
            internal static int LibraryFolderIn(string moduleRoot, PsStr16 name, ushort* buf, nuint cap, nuint* len, IntPtr* err)
            {
                try { return WriteChars(NativeModule.StageFolder(moduleRoot, name.ToString()), buf, cap, len); }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// lifecycle_removal: the removal the calling thread is running a
            /// module's removal hook for, as <see cref="NativeModule"/> records
            /// it around the hook. The runspace's InstanceId goes out as the
            /// bytes Guid.ToByteArray gives.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int LifecycleRemoval(uint* cause, byte* runspace, byte* hasRunspace, IntPtr* err)
            {
                try
                {
                    NativeModule.Removal? removal = NativeModule.CurrentRemoval;
                    if (removal == null)
                    {
                        return Fail(new InvalidOperationException("pwrs: lifecycle_removal was called on a thread running no removal hook"), err);
                    }
                    *cause = removal.Cause;
                    if (removal.Runspace is Guid id)
                    {
                        byte[] b = id.ToByteArray();
                        for (int i = 0; i < 16; i++) runspace[i] = b[i];
                        *hasRunspace = 1;
                    }
                    else
                    {
                        *hasRunspace = 0;
                    }
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            // ---- the events of objects ----

            /// <summary>
            /// object_event_raise: raises the event <paramref name="name"/> of
            /// a generated proxy object with what <paramref name="args"/>
            /// gives, as <see cref="ObjectEvents.Raise"/> does: an array's
            /// elements, none for null, and anything else as the one
            /// argument. A handler's exception comes back through err.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ObjectEventRaise(IntPtr obj, PsStr16 name, IntPtr args, IntPtr* err)
            {
                try
                {
                    object target = Base(obj) ?? throw new ArgumentNullException(nameof(obj), "pwrs: an event of $null was raised");
                    ObjectEvents.Raise(target, name.ToString(), Args(args));
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            /// <summary>The COM event handlers of the shared table's module: on .NET, this copy's.</summary>
            private static ComHooks SharedComHooks => ComHooks.OfThisCopy;
#else
            /// <summary>
            /// The COM event handlers of the shared table, which no module's
            /// library receives on .NET Framework, where each has a copy of
            /// its own whose object_event_subscribe records them in the module's.
            /// </summary>
            private static readonly ComHooks SharedComHooks = new ComHooks();
#endif

            /// <summary>
            /// object_event_subscribe: subscribes the native callback to the
            /// event <paramref name="name"/> of <paramref name="source"/>, an
            /// object, or a type for its static event, and returns the
            /// subscription through <paramref name="out"/>. On success the
            /// subscription owns <paramref name="context"/> and calls
            /// <paramref name="release"/> with it once; on failure the
            /// caller still owns it.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ObjectEventSubscribe(IntPtr source, PsStr16 name, IntPtr callback, IntPtr context, IntPtr release, IntPtr* @out, IntPtr* err) =>
                ObjectEventSubscribeIn(SharedComHooks, source, name, callback, context, release, @out, err);

            /// <summary>
            /// object_event_subscribe for the module whose COM event handlers
            /// are <paramref name="hooks"/>, which record a handler attached to a
            /// COM object until it is detached.
            /// </summary>
            internal static int ObjectEventSubscribeIn(ComHooks hooks, IntPtr source, PsStr16 name, IntPtr callback, IntPtr context, IntPtr release, IntPtr* @out, IntPtr* err)
            {
                try
                {
                    object target = Base(source) ?? throw new ArgumentNullException(nameof(source), "pwrs: an event of $null was subscribed to");
                    *@out = Alloc(new Subscription(target, name.ToString(), callback, context, release, hooks));
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// object_event_unsubscribe: removes a subscription's handler, as
            /// <see cref="Subscription.Remove"/> does, without waiting for a
            /// call running on another thread. The handle stays the caller's
            /// to free.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ObjectEventUnsubscribe(IntPtr subscription, IntPtr* err)
            {
                try
                {
                    if (!(Target(subscription) is Subscription s)) throw new ArgumentException("pwrs: object_event_unsubscribe was given something other than a subscription");
                    s.Remove();
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// proxy_exit_raising: ends a borrow proxy_enter began, and when it
            /// was the thread's last entry delivers the raises made during
            /// its entries, as <see cref="ProxyCore.ExitBorrowRaising"/> does;
            /// a handler's exception comes back through err.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ProxyExitRaising(IntPtr obj, byte changed, IntPtr* err)
            {
                try
                {
                    if (Base(obj) is IProxy p) p.PwrsCore.ExitBorrowRaising(changed != 0);
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// <see cref="MemoryView"/> over a Rust buffer, writable and
            /// never refused, released by drop(ptr).
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int MemoryViewNew(uint tag, void* ptr, nuint len, IntPtr drop, IntPtr* @out, IntPtr* err)
            {
                try
                {
                    *@out = MemoryView(tag, ptr, len, false, null, ptr, drop);
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// <see cref="MemoryView"/> over memory whose owner the module
            /// passes as context and releases through release.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int MemoryViewBorrow(uint tag, void* ptr, nuint len, byte readOnly, byte* refused, void* context, IntPtr release, IntPtr* @out, IntPtr* err)
            {
                try
                {
                    *@out = MemoryView(tag, ptr, len, readOnly != 0, refused, context, release);
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// A handle to a view of len elements at ptr. On .NET it is a
            /// Memory&lt;T&gt;, or a ReadOnlyMemory&lt;T&gt;, over that memory,
            /// and release(context) runs when its manager is disposed or
            /// collected. On .NET Framework the elements are copied into a
            /// T[] and release(context) runs before this returns. A view
            /// whose refusal byte is set is refused here. When this
            /// throws, release has not run and never will.
            /// </summary>
            private static IntPtr MemoryView(uint tag, void* ptr, nuint len, bool readOnly, byte* refused, void* context, IntPtr release)
            {
                int n = checked((int)len);
                if (refused != null && System.Threading.Volatile.Read(ref *refused) != 0) throw MemoryRevoked();
#if NET
                var rel = (delegate* unmanaged[Cdecl]<void*, void>)release;
                return tag switch
                {
                    Native.TypeU8 => View<byte>(ptr, n, readOnly, refused, context, rel),
                    Native.TypeI8 => View<sbyte>(ptr, n, readOnly, refused, context, rel),
                    Native.TypeI16 => View<short>(ptr, n, readOnly, refused, context, rel),
                    Native.TypeU16 => View<ushort>(ptr, n, readOnly, refused, context, rel),
                    Native.TypeI32 => View<int>(ptr, n, readOnly, refused, context, rel),
                    Native.TypeU32 => View<uint>(ptr, n, readOnly, refused, context, rel),
                    Native.TypeI64 => View<long>(ptr, n, readOnly, refused, context, rel),
                    Native.TypeU64 => View<ulong>(ptr, n, readOnly, refused, context, rel),
                    Native.TypeF32 => View<float>(ptr, n, readOnly, refused, context, rel),
                    Native.TypeF64 => View<double>(ptr, n, readOnly, refused, context, rel),
                    _ => throw new ArgumentOutOfRangeException(nameof(tag), $"memory views need a primitive element tag, got {tag}"),
                };
#else
                Type elem = Native.ElementType(tag);
                if (!elem.IsPrimitive || elem == typeof(bool) || elem == typeof(char)) throw new ArgumentOutOfRangeException(nameof(tag), $"memory views need a primitive element tag, got {tag}");
                Array copy = Array.CreateInstance(elem, n);
                long bytes = (long)n * Marshal.SizeOf(elem);
                var pin = GCHandle.Alloc(copy, GCHandleType.Pinned);
                try { Buffer.MemoryCopy(ptr, (void*)pin.AddrOfPinnedObject(), bytes, bytes); }
                finally { pin.Free(); }
                IntPtr handle = Alloc(copy);
                if (release != IntPtr.Zero) Marshal.GetDelegateForFunctionPointer<DropFn>(release)(context);
                return handle;
#endif
            }

            /// <summary>
            /// Finishes a task's source: with the object value for outcome
            /// 0, faulted for 1 with value the exception a failing export
            /// returns or its "[id] message" text, canceled for 2. Only the
            /// first call takes effect.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int TaskSettle(IntPtr source, uint outcome, IntPtr value, IntPtr* err)
            {
                try
                {
                    var task = (TaskSource)Target(source)!;
                    switch (outcome)
                    {
                        case Native.TaskValue:
                            task.SetValue(Target(value));
                            break;
                        case Native.TaskFault:
                            object? error = Target(value);
                            task.SetFault(error as Exception ?? new PwrsException(error as string ?? "the module failed the task without saying why"));
                            break;
                        default:
                            task.SetCancel();
                            break;
                    }
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// Raises an engine event through a PSEventManager with the
            /// arguments New-Event passes: the sender and the arguments as
            /// their base objects, the message data as a PSObject, and the
            /// event processed on this thread without waiting for its
            /// actions, which run on the runspace's pipeline thread.
            /// Processing it on this thread is what preserves the order one
            /// thread raises in.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int EventRaise(IntPtr events, PsStr16 sourceIdentifier, IntPtr sender, IntPtr args, IntPtr messageData, IntPtr* err)
            {
                try
                {
                    var manager = (PSEventManager)Base(events)!;
                    object? data = Target(messageData);
                    manager.GenerateEvent(sourceIdentifier.ToString(), Base(sender), Args(args), data == null ? null : PSObject.AsPSObject(data), true, false);
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// Writes to the host the way Write-Host does: a
            /// HostInformationMessage written to the information stream
            /// with the PSHOST tag, so the engine shows it whatever the
            /// information preference, a 6> redirection takes it, and a
            /// transcript records it. A color of -1 is the one the host's
            /// RawUI reports, which is what Write-Host's
            /// ConsoleColorCmdlet reads for a color it is not given; a
            /// host with no RawUI throws HostException there, and
            /// Write-Host leaves both colors unset, as this does.
            /// Windows PowerShell 5.1 transcribes no information record,
            /// so its Write-Host passes the text to the transcript itself
            /// after the record, through the host's TranscribeResult, and
            /// the netstandard2.0 half does the same. TranscribeResult is
            /// internal, so it is reached through reflection.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteHost(IntPtr c, PsStr16 text, int foreground, int background, byte noNewLine, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                if (foreground < -1 || foreground > 15) return Fail(new ArgumentOutOfRangeException(nameof(foreground), foreground, "a console color is 0 to 15, or -1 for the host's current color"), err);
                if (background < -1 || background > 15) return Fail(new ArgumentOutOfRangeException(nameof(background), background, "a console color is 0 to 15, or -1 for the host's current color"), err);
                try
                {
                    var message = new HostInformationMessage { Message = text.ToString(), NoNewLine = noNewLine != 0 };
                    try
                    {
                        message.ForegroundColor = foreground >= 0 ? (ConsoleColor)foreground : cmd.Host.UI.RawUI.ForegroundColor;
                        message.BackgroundColor = background >= 0 ? (ConsoleColor)background : cmd.Host.UI.RawUI.BackgroundColor;
                    }
                    catch (System.Management.Automation.Host.HostException)
                    {
                        // A host with no RawUI, as Write-Host meets it.
                    }
                    cmd.WriteInformation(message, new[] { "PSHOST" });
#if !NET
                    TranscribeResult?.Invoke(cmd.Host.UI, new object[] { message.Message });
#endif
                    return Native.Ok;
                }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

#if !NET
            /// <summary>
            /// PSHostUserInterface.TranscribeResult(string), which records
            /// a line in the session's transcript; internal in Windows
            /// PowerShell 5.1.
            /// </summary>
            private static readonly MethodInfo? TranscribeResult = typeof(System.Management.Automation.Host.PSHostUserInterface).GetMethod(
                "TranscribeResult", BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic, null, new[] { typeof(string) }, null);
#endif

#if NET
            /// <summary>
            /// The manager's finalizer would release a view whose handle
            /// was never made, so a failed Alloc takes it off the queue.
            /// </summary>
            private static IntPtr View<T>(void* ptr, int n, bool readOnly, byte* refused, void* context, delegate* unmanaged[Cdecl]<void*, void> release) where T : unmanaged
            {
                var manager = new RustMemoryManager<T>(ptr, n, refused, context, release);
                try { return readOnly ? Alloc((ReadOnlyMemory<T>)manager.Memory) : Alloc(manager.Memory); }
                catch
                {
                    GC.SuppressFinalize(manager);
                    throw;
                }
            }
#endif

            // ---- direct scalar writes ----
            // One call and no GCHandle. The generic route costs
            // string_new plus write_object plus free_handle.

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteString(IntPtr c, PsStr16 text, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try { cmd.WriteObject(text.ToString()); return Native.Ok; }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteI64(IntPtr c, long v, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try { cmd.WriteObject(v); return Native.Ok; }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteF64(IntPtr c, double v, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try { cmd.WriteObject(v); return Native.Ok; }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteBool(IntPtr c, byte v, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                if (Refused(cmd, err, out int refused)) return refused;
                try { cmd.WriteObject(v != 0); return Native.Ok; }
                catch (Exception e) { return FailThrough(cmd, e, err); }
            }

            // ---- diagnostics ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ExceptionDescribe(IntPtr errHandle, ushort* buf, nuint cap, nuint* len)
            {
                object? o = Target(errHandle);
                string s = o is Exception e ? e.GetType().Name + ": " + e.Message : o?.ToString() ?? string.Empty;
                return WriteChars(s, buf, cap, len);
            }

            /// <summary>
            /// An exception whose Message is <paramref name="message"/> and
            /// whose InnerException's Message is <paramref name="details"/>.
            /// A native entry point returns it through its err
            /// out-parameter, and the call site throws it as it is.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr ExceptionNew(PsStr16 message, PsStr16 details) =>
                Alloc(new PwrsException(message.ToString(), new PwrsException(details.ToString())));

            /// <summary>
            /// An exception of the type <paramref name="type"/> names, as
            /// <see cref="NamedException.Make"/> makes it. A native entry point
            /// returns it through its err out-parameter, and the call site
            /// throws it as it is; a record path passes it to
            /// <see cref="WriteErrorException"/> or
            /// <see cref="JobWriteErrorException"/>.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr ExceptionNamed(PsStr16 type, PsStr16 message, PsStr16 id, uint category, PsStr16 details) =>
                Alloc(NamedException.Make(type.ToString(), message.ToString(), id.ToString(), (ErrorCategory)category, details.ToString()));

            // ---- unsigned 64-bit primitives ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr U64New(ulong v) => Alloc(v);

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int U64Read(IntPtr h, ulong* v, IntPtr* err)
            {
                try { *v = LanguagePrimitives.ConvertTo<ulong>(Target(h)); return Native.Ok; }
                catch (Exception e) { return Fail(e, err); }
            }

            // ---- dates, time spans, GUIDs, chars ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int DateTimeNew(long ticks, byte kind, IntPtr* @out, IntPtr* err)
            {
                try { *@out = Alloc(new DateTime(ticks, (DateTimeKind)kind)); return Native.Ok; }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int DateTimeRead(IntPtr h, long* ticks, byte* kind, IntPtr* err)
            {
                try
                {
                    DateTime d = LanguagePrimitives.ConvertTo<DateTime>(Target(h));
                    *ticks = d.Ticks;
                    *kind = (byte)d.Kind;
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr TimeSpanNew(long ticks) => Alloc(new TimeSpan(ticks));

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int TimeSpanRead(IntPtr h, long* ticks, IntPtr* err)
            {
                try { *ticks = LanguagePrimitives.ConvertTo<TimeSpan>(Target(h)).Ticks; return Native.Ok; }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr GuidNew(byte* bytes)
            {
                var copy = new byte[16];
                for (int i = 0; i < 16; i++) copy[i] = bytes[i];
                return Alloc(new Guid(copy));
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int GuidRead(IntPtr h, byte* bytes, IntPtr* err)
            {
                try
                {
                    byte[] b = LanguagePrimitives.ConvertTo<Guid>(Target(h)).ToByteArray();
                    for (int i = 0; i < 16; i++) bytes[i] = b[i];
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr CharNew(ushort unit) => Alloc((char)unit);

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int CharRead(IntPtr h, ushort* unit, IntPtr* err)
            {
                try { *unit = LanguagePrimitives.ConvertTo<char>(Target(h)); return Native.Ok; }
                catch (Exception e) { return Fail(e, err); }
            }

            // ---- secure strings ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int SecureStringNew(PsStr16 text, IntPtr* @out, IntPtr* err)
            {
                try
                {
                    var s = text.Len == 0 ? new SecureString() : new SecureString((char*)text.Ptr, checked((int)text.Len));
                    s.MakeReadOnly();
                    *@out = Alloc(s);
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// Decrypts into an unmanaged buffer, copies what fits into
            /// buf, reports the full length, and zeroes that buffer
            /// before returning. A cap of 0 reports the length only.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int SecureStringRead(IntPtr h, ushort* buf, nuint cap, nuint* len, IntPtr* err)
            {
                try
                {
                    if (!(Base(h) is SecureString s)) throw new ArgumentException("pwrs: value is not a SecureString");
                    if (len != null) *len = (nuint)s.Length;
                    if (cap == 0 || buf == null) return Native.Ok;
                    IntPtr plain = Marshal.SecureStringToGlobalAllocUnicode(s);
                    try
                    {
                        int n = (int)Math.Min((ulong)s.Length, (ulong)cap);
                        Buffer.MemoryCopy((void*)plain, buf, (long)cap * 2, (long)n * 2);
                    }
                    finally { Marshal.ZeroFreeGlobalAllocUnicode(plain); }
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            // ---- typed arrays ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ArrayElementTag(IntPtr h, uint* tag, IntPtr* err)
            {
                try
                {
                    *tag = Base(h) is Array a ? Native.ElementTag(a.GetType().GetElementType()!) : Native.TypeObject;
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int StreamEnabled(IntPtr c, uint kind, byte* enabled, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try
                {
                    *enabled = (byte)(cmd.StreamEnabled(kind) ? 1 : 0);
                    return Native.Ok;
                }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

            // ---- read-only views ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ReadOnlyTableNew(IntPtr source, IntPtr* @out, IntPtr* err)
            {
                try
                {
                    // A PSObject arrives wrapped, and the engine passes
                    // a script's hashtable that way.
                    object? o = Base(source);
                    if (o is not IDictionary d)
                    {
                        throw new ArgumentException($"pwrs: a read-only table needs an IDictionary, not {o?.GetType().FullName ?? "null"}");
                    }
                    *@out = Alloc(new ReadOnlyTable(d));
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            // ---- object type tag ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ObjectTypeTag(IntPtr h, uint* tag, IntPtr* err)
            {
                try
                {
                    // Base unwraps a PSObject, so a value the engine
                    // wrapped gets the tag of what it wraps.
                    object? o = Base(h);
                    *tag = o is null ? Native.TypeObject : Native.ElementTag(o.GetType());
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            // ---- scalars at their own width ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr I8New(sbyte v) => Alloc(v);

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr I16New(short v) => Alloc(v);

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr I32New(int v) => Alloc(v);

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr U8New(byte v) => Alloc(v);

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr U16New(ushort v) => Alloc(v);

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr U32New(uint v) => Alloc(v);

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static IntPtr F32New(float v) => Alloc(v);

            // ---- decimal ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int DecimalNew(int lo, int mid, int hi, int flags, IntPtr* @out, IntPtr* err)
            {
                try
                {
                    // The four-int constructor validates flags and
                    // raises on a scale above 28 or reserved bits set.
                    *@out = Alloc(new decimal(new[] { lo, mid, hi, flags }));
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int DecimalRead(IntPtr h, int* bits, IntPtr* err)
            {
                try
                {
                    decimal d = LanguagePrimitives.ConvertTo<decimal>(Target(h));
                    int[] words = decimal.GetBits(d);
                    for (int i = 0; i < 4; i++) bits[i] = words[i];
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            // ---- date and time with an offset ----

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int DateTimeOffsetNew(long ticks, short offsetMinutes, IntPtr* @out, IntPtr* err)
            {
                try
                {
                    var offset = TimeSpan.FromMinutes(offsetMinutes);
                    *@out = Alloc(new DateTimeOffset(ticks, offset));
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int DateTimeOffsetRead(IntPtr h, long* ticks, short* offsetMinutes, IntPtr* err)
            {
                try
                {
                    var v = LanguagePrimitives.ConvertTo<DateTimeOffset>(Target(h));
                    *ticks = v.Ticks;
                    *offsetMinutes = (short)(v.Offset.Ticks / TimeSpan.TicksPerMinute);
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }
        }
    }
}
