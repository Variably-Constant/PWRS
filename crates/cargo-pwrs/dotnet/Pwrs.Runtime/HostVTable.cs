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
    /// Builds the function table handed to the native module. Layout
    /// mirrors pwrs_sys::HostVTable field for field: an 8-byte header
    /// (size, version) then one pointer per entry in declaration
    /// order. The table is allocated once in unmanaged memory and
    /// never freed. On net10.0 entries are UnmanagedCallersOnly
    /// statics; on netstandard2.0 they are delegates rooted in Roots.
    ///
    /// On net10.0 every module has a copy of this assembly of its own,
    /// so <see cref="Pointer"/> is handed to one module. On
    /// netstandard2.0 one copy serves every module in the process, and
    /// each module is handed a <c>ModuleTable</c>: this table with
    /// factory_new, proxy_enter, proxy_enter_shared and helper_path
    /// answered for that module.
    /// </summary>
    public static unsafe class HostVTable
    {
        /// Function-pointer slots, matching the native HostVTable field
        /// count exactly so the size header equals size_of::&lt;HostVTable&gt;().
        public const int SlotCount = 76;

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
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void DropFn(void* p);

        /// <summary>
        /// The host table one module's native library is handed on .NET
        /// Framework: a copy of <see cref="HostVTable.Pointer"/> whose
        /// factory_new, proxy_enter and proxy_enter_shared answer from
        /// that module's factories and whose helper_path stages from that
        /// module's folder. Class ids are only unique within a module,
        /// and one copy of this assembly serves every module in the
        /// process, so the shared table cannot tell whose class an id
        /// names or whose helper a name means.
        ///
        /// The copy holds the addresses of the delegates behind those
        /// four entries, so the delegates live as long as this object,
        /// which the module's NativeModule keeps for the life of the
        /// process. Like the shared table, the copy is never freed.
        /// </summary>
        internal sealed class ModuleTable
        {
            internal readonly IntPtr Pointer;
            private readonly FactoryNewFn _factoryNew;
            private readonly ProxyEnterFn _proxyEnter;
            private readonly ProxyEnterFn _proxyEnterShared;
            private readonly HelperPathFn _helperPath;

            internal ModuleTable(Factories factories, string moduleRoot)
            {
                _factoryNew = (classId, fields, @out, err) => Entries.FactoryNewIn(factories, classId, fields, @out, err);
                _proxyEnter = (obj, classId, instance, err) => Entries.ProxyEnterIn(factories, obj, classId, instance, err, true);
                _proxyEnterShared = (obj, classId, instance, err) => Entries.ProxyEnterIn(factories, obj, classId, instance, err, false);
                _helperPath = (name, buf, cap, len, err) => Entries.HelperPathIn(moduleRoot, name, buf, cap, len, err);
                int bytes = 8 + SlotCount * IntPtr.Size;
                Pointer = Marshal.AllocHGlobal(bytes);
                Buffer.MemoryCopy((void*)HostVTable.Pointer, (void*)Pointer, bytes, bytes);
                Marshal.WriteIntPtr(Pointer, 8 + _factoryNewSlot * IntPtr.Size, Marshal.GetFunctionPointerForDelegate(_factoryNew));
                Marshal.WriteIntPtr(Pointer, 8 + _proxyEnterSlot * IntPtr.Size, Marshal.GetFunctionPointerForDelegate(_proxyEnter));
                Marshal.WriteIntPtr(Pointer, 8 + _proxyEnterSharedSlot * IntPtr.Size, Marshal.GetFunctionPointerForDelegate(_proxyEnterShared));
                Marshal.WriteIntPtr(Pointer, 8 + _helperPathSlot * IntPtr.Size, Marshal.GetFunctionPointerForDelegate(_helperPath));
            }
        }
#endif

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
            private static int Fail(Exception e, IntPtr* err)
            {
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
                try { cmd.WriteObject(Target(obj), enumerate != 0); return Native.Ok; }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteError(IntPtr c, PsStr16 message, PsStr16 id, uint category, IntPtr target, byte terminating, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try
                {
                    var record = new ErrorRecord(new PwrsException(message.ToString()), id.ToString(), (ErrorCategory)category, Target(target));
                    if (terminating != 0) cmd.SetPendingTerminating(record);
                    else cmd.WriteError(record);
                    return Native.Ok;
                }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteStream(IntPtr c, uint kind, PsStr16 text, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try
                {
                    string s = text.ToString();
                    switch (kind)
                    {
                        case Native.StreamVerbose: cmd.WriteVerbose(s); break;
                        case Native.StreamDebug: cmd.WriteDebug(s); break;
                        case Native.StreamWarning: cmd.WriteWarning(s); break;
                        case Native.StreamInformation: cmd.WriteInformation(s, null); break;
                        default: throw new ArgumentOutOfRangeException(nameof(kind), $"unknown stream kind {kind}");
                    }
                    return Native.Ok;
                }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteProgress(IntPtr c, int id, PsStr16 activity, PsStr16 status, int percent, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
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

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ShouldProcess(IntPtr c, PsStr16 target, PsStr16 action, byte* yes, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try { *yes = (byte)(cmd.ShouldProcess(target.ToString(), action.ToString()) ? 1 : 0); return Native.Ok; }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ShouldContinue(IntPtr c, PsStr16 query, PsStr16 caption, byte* yes, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try { *yes = (byte)(cmd.ShouldContinue(query.ToString(), caption.ToString()) ? 1 : 0); return Native.Ok; }
                catch (Exception e) { return FailOn(cmd, e, err); }
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
                *bound = (byte)(cmd.MyInvocation.BoundParameters.ContainsKey(name.ToString()) ? 1 : 0);
                return Native.Ok;
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
                    string n = name.ToString();
                    // Resolved by name and added as a CommandInfo, so no
                    // script text is built or parsed on the way.
                    CommandInfo? info = cmd.SessionState.InvokeCommand.GetCommand(n, CommandTypes.All);
                    if (info == null) throw new ArgumentException($"pwrs: no command named {n}");
                    using (var ps = PowerShell.Create(RunspaceMode.CurrentRunspace))
                    {
                        ps.AddCommand(info);
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
                        // run it, and its output is kept.
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
            /// A table answers its own get_Item, set_Item, ContainsKey,
            /// Contains and Remove, through the IDictionary it is, so a
            /// PsHashtable read pays neither the adapter's overload
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
                    object? r = t.InvokeMember(name.ToString(), BindingFlags.Public | BindingFlags.Static | BindingFlags.InvokeMethod, null, null, Args(args));
                    *@out = Alloc(r);
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
                    *@out = Alloc(Activator.CreateInstance(t, Args(args)));
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            // ---- generated types ----

            /// <summary>
            /// factory_new in the shared table. On .NET it answers from
            /// the one table of the shell this copy of the assembly
            /// serves. On .NET Framework each module's native library is
            /// handed a <c>ModuleTable</c> instead, so a call reaching
            /// this entry came through a table no module owns, and is
            /// refused rather than answered from a guess.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int FactoryNew(uint classId, void* fields, IntPtr* @out, IntPtr* err)
            {
#if NET
                return FactoryNewIn(Pwrs.Factories.OfThisCopy, classId, fields, @out, err);
#else
                return Fail(new InvalidOperationException($"pwrs: factory_new for class id {classId} reached the shared host table, which no module is handed on .NET Framework"), err);
#endif
            }

            /// <summary>factory_new answered from one module's factories.</summary>
            internal static int FactoryNewIn(Factories factories, uint classId, void* fields, IntPtr* @out, IntPtr* err)
            {
                try { *@out = Alloc(factories.Create(classId, (IntPtr)fields)); return Native.Ok; }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// proxy_enter in the shared table, which on .NET Framework no
            /// module is handed; see <see cref="FactoryNew"/>.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ProxyEnter(IntPtr obj, uint classId, IntPtr* instance, IntPtr* err)
            {
#if NET
                return ProxyEnterIn(Pwrs.Factories.OfThisCopy, obj, classId, instance, err, true);
#else
                return Fail(new InvalidOperationException($"pwrs: proxy_enter for class id {classId} reached the shared host table, which no module is handed on .NET Framework"), err);
#endif
            }

            /// <summary>
            /// proxy_enter_shared in the shared table: the shared form of
            /// <see cref="ProxyEnter"/>, for a `with` borrow, which nests
            /// inside a `&self` method or a property read. On .NET
            /// Framework no module is handed the shared table.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int ProxyEnterShared(IntPtr obj, uint classId, IntPtr* instance, IntPtr* err)
            {
#if NET
                return ProxyEnterIn(Pwrs.Factories.OfThisCopy, obj, classId, instance, err, false);
#else
                return Fail(new InvalidOperationException($"pwrs: proxy_enter_shared for class id {classId} reached the shared host table, which no module is handed on .NET Framework"), err);
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
                    if (!(o is ProxyBase p))
                    {
                        string got = o == null ? "$null" : "a " + o.GetType().FullName;
                        throw new PwrsException($"a proxy object of this module was expected, and {got} was passed");
                    }
                    *instance = p.EnterBorrow(factories, classId, exclusive);
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
                if (Base(obj) is ProxyBase p) p.ExitBorrow(changed != 0);
            }

            // ---- helper executables ----

            /// <summary>
            /// helper_path in the shared table. On .NET it stages from the
            /// folder of the module this copy of the assembly serves; on
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
                return Fail(new InvalidOperationException($"pwrs: helper_path for {name} reached the shared host table, which no module is handed on .NET Framework"), err);
#endif
            }

            /// <summary>helper_path staged from one module's folder.</summary>
            internal static int HelperPathIn(string moduleRoot, PsStr16 name, ushort* buf, nuint cap, nuint* len, IntPtr* err)
            {
                try { return WriteChars(NativeModule.StageHelper(moduleRoot, name.ToString()), buf, cap, len); }
                catch (Exception e) { return Fail(e, err); }
            }

            /// <summary>
            /// On .NET the view is a Memory&lt;T&gt; over the Rust buffer and
            /// drop runs when it is collected. On .NET Framework the
            /// bytes are copied into a managed array and drop runs
            /// before this returns.
            /// </summary>
#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int MemoryViewNew(uint tag, void* ptr, nuint len, IntPtr drop, IntPtr* @out, IntPtr* err)
            {
                try
                {
                    int n = checked((int)len);
#if NET
                    var dropFn = (delegate* unmanaged[Cdecl]<void*, void>)drop;
                    object view = tag switch
                    {
                        Native.TypeU8 => new RustMemoryManager<byte>(ptr, n, dropFn).Memory,
                        Native.TypeI8 => new RustMemoryManager<sbyte>(ptr, n, dropFn).Memory,
                        Native.TypeI16 => new RustMemoryManager<short>(ptr, n, dropFn).Memory,
                        Native.TypeU16 => new RustMemoryManager<ushort>(ptr, n, dropFn).Memory,
                        Native.TypeI32 => new RustMemoryManager<int>(ptr, n, dropFn).Memory,
                        Native.TypeU32 => new RustMemoryManager<uint>(ptr, n, dropFn).Memory,
                        Native.TypeI64 => new RustMemoryManager<long>(ptr, n, dropFn).Memory,
                        Native.TypeU64 => new RustMemoryManager<ulong>(ptr, n, dropFn).Memory,
                        Native.TypeF32 => new RustMemoryManager<float>(ptr, n, dropFn).Memory,
                        Native.TypeF64 => new RustMemoryManager<double>(ptr, n, dropFn).Memory,
                        _ => throw new ArgumentOutOfRangeException(nameof(tag), $"memory views need a primitive element tag, got {tag}"),
                    };
                    *@out = Alloc(view);
#else
                    Type elem = Native.ElementType(tag);
                    if (!elem.IsPrimitive || elem == typeof(bool) || elem == typeof(char)) throw new ArgumentOutOfRangeException(nameof(tag), $"memory views need a primitive element tag, got {tag}");
                    Array copy = Array.CreateInstance(elem, n);
                    long bytes = (long)n * Marshal.SizeOf(elem);
                    var pin = GCHandle.Alloc(copy, GCHandleType.Pinned);
                    try { Buffer.MemoryCopy(ptr, (void*)pin.AddrOfPinnedObject(), bytes, bytes); }
                    finally { pin.Free(); }
                    if (drop != IntPtr.Zero) Marshal.GetDelegateForFunctionPointer<DropFn>(drop)(ptr);
                    *@out = Alloc(copy);
#endif
                    return Native.Ok;
                }
                catch (Exception e) { return Fail(e, err); }
            }

            // ---- direct scalar writes ----
            // One crossing and no GCHandle. The generic route costs
            // string_new plus write_object plus free_handle.

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteString(IntPtr c, PsStr16 text, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try { cmd.WriteObject(text.ToString()); return Native.Ok; }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteI64(IntPtr c, long v, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try { cmd.WriteObject(v); return Native.Ok; }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteF64(IntPtr c, double v, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try { cmd.WriteObject(v); return Native.Ok; }
                catch (Exception e) { return FailOn(cmd, e, err); }
            }

#if NET
            [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
#endif
            internal static int WriteBool(IntPtr c, byte v, IntPtr* err)
            {
                var cmd = Cmdlet(c);
                if (OffThread(cmd, err)) return Native.ErrWrongThread;
                try { cmd.WriteObject(v != 0); return Native.Ok; }
                catch (Exception e) { return FailOn(cmd, e, err); }
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
                    // A PSObject arrives wrapped, and the engine hands
                    // a script's hashtable over that way.
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
                    // wrapped answers the tag of what it wraps.
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
