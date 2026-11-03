using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using System.Linq.Expressions;
using System.Management.Automation;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Threading;

namespace Pwrs
{
    /// <summary>
    /// The events of .NET objects as Rust reaches them: an event of a
    /// generated proxy class raised from Rust, and a Rust callback
    /// subscribed to an event of any object or type.
    ///
    /// A generated class stores each event's handlers in a field of its own,
    /// named by <see cref="FieldName"/>, and declares nothing else for its
    /// events, so it needs nothing of this assembly that an older runtime
    /// lacks. A raise reads that field.
    ///
    /// A raise the thread inside a call into the object makes, from a
    /// method, a property read or a borrow, is queued on the object's
    /// <see cref="ProxyCore"/> and delivered when that thread's last entry
    /// leaves, so a handler can read and call the object, which the call
    /// would refuse it while it has the object borrowed. Any other raise is
    /// delivered before it returns. Either way the handlers run on the
    /// thread that raised, as a C# raise runs them.
    /// </summary>
    internal static class ObjectEvents
    {
        private const string FieldPrefix = "__pwrsEvent_";

        private static readonly ConcurrentDictionary<(Type, string), FieldInfo?> s_fields = new ConcurrentDictionary<(Type, string), FieldInfo?>();
        private static readonly ConcurrentDictionary<Type, Shape> s_shapes = new ConcurrentDictionary<Type, Shape>();

        /// <summary>
        /// The field a generated class stores the handlers of the event it
        /// raises by <paramref name="raise"/> in: the prefix and the name,
        /// each character other than an ASCII letter, digit or _ written as
        /// _. cargo pwrs names the field by the same rule.
        /// </summary>
        internal static string FieldName(string raise)
        {
            var name = new char[raise.Length];
            for (int i = 0; i < raise.Length; i++)
            {
                char ch = raise[i];
                bool kept = (ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') || (ch >= '0' && ch <= '9') || ch == '_';
                name[i] = kept ? ch : '_';
            }
            return FieldPrefix + new string(name);
        }

        /// <summary>
        /// Raises the event <paramref name="name"/> of <paramref name="target"/>,
        /// an object of a generated proxy class, with the arguments Rust
        /// gives: for a delegate (object sender, T e) the object is the
        /// sender and the one argument is e, and for any other delegate the
        /// arguments are all of its own. Each converts to its parameter's
        /// type as PowerShell converts it, here, so a raise that cannot be
        /// delivered fails where it is made, queued or not.
        /// </summary>
        internal static void Raise(object target, string name, object?[] args)
        {
            if (!(target is IProxy proxy))
            {
                throw new PwrsException($"a {target.GetType().FullName} is not an object of a module's proxy class, and only such an object raises its events through PWRS");
            }
            Type type = target.GetType();
            FieldInfo? field = s_fields.GetOrAdd((type, name), key => DeclaredField(key.Item1, FieldName(key.Item2)));
            if (field == null || !typeof(Delegate).IsAssignableFrom(field.FieldType))
            {
                throw new PwrsException($"{type.FullName} has no event {name} to raise; it raises {EventNames(type)}");
            }
            Shape shape = ShapeOf(field.FieldType);
            object?[] invokeArgs = shape.Arguments(name, target, args);
            var handlers = (Delegate?)field.GetValue(target);
            if (handlers == null) return;
            if (proxy.PwrsCore.Queue(new QueuedRaise(name, target, handlers, shape, invokeArgs))) return;
            shape.Invoke(handlers, invokeArgs);
        }

        /// <summary>
        /// The private field named <paramref name="field"/> that <paramref name="type"/>
        /// or one of its bases declares: a script class deriving from a module's class
        /// does not reach the base's private fields through its own type.
        /// </summary>
        private static FieldInfo? DeclaredField(Type type, string field)
        {
            for (Type? t = type; t != null; t = t.BaseType)
            {
                FieldInfo? found = t.GetField(field, BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.DeclaredOnly);
                if (found != null) return found;
            }
            return null;
        }

        /// <summary>The events of <paramref name="type"/> and its bases a raise can name, for a message.</summary>
        private static string EventNames(Type type)
        {
            var names = new List<string>();
            for (Type? t = type; t != null; t = t.BaseType)
            {
                foreach (EventInfo e in t.GetEvents(BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.DeclaredOnly))
                {
                    string raise = e.Name;
                    int last = raise.LastIndexOf('.');
                    if (last > 0)
                    {
                        int before = raise.LastIndexOf('.', last - 1);
                        raise = raise.Substring(before + 1);
                    }
                    if (t.GetField(FieldName(raise), BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.DeclaredOnly) != null) names.Add(raise);
                }
            }
            return names.Count == 0 ? "none" : string.Join(", ", names);
        }

        internal static Shape ShapeOf(Type delegateType) => s_shapes.GetOrAdd(delegateType, t => new Shape(t));

        /// <summary>
        /// How one delegate type is invoked with what Rust gives, and what it
        /// gives a callback: its parameters, whether it takes the sender
        /// first, and an invoker compiled for it, through which a handler's
        /// exception arrives as the handler threw it.
        /// </summary>
        internal sealed class Shape
        {
            internal readonly Type Delegate;
            internal readonly Type[] Parameters;
            internal readonly Type Returns;
            /// <summary>The delegate takes (object sender, T e).</summary>
            internal readonly bool SenderFirst;
            /// <summary>Why no Rust code raises or receives it, or null.</summary>
            private readonly string? _refused;
            private readonly Action<Delegate, object?[]>? _invoke;

            internal Shape(Type delegateType)
            {
                Delegate = delegateType;
                MethodInfo invoke = delegateType.GetMethod("Invoke") ?? throw new PwrsException($"{delegateType.FullName} is not a delegate type");
                ParameterInfo[] parameters = invoke.GetParameters();
                Parameters = parameters.Select(p => p.ParameterType).ToArray();
                Returns = invoke.ReturnType;
                SenderFirst = Parameters.Length == 2 && Parameters[0] == typeof(object);
                if (Parameters.Any(p => p.IsByRef))
                {
                    _refused = "takes a ref, out or in argument, which PWRS does not pass";
                    return;
                }
                ParameterExpression d = Expression.Parameter(typeof(Delegate), "d");
                ParameterExpression a = Expression.Parameter(typeof(object[]), "a");
                Expression[] each = Parameters.Select((p, i) => (Expression)Expression.Convert(Expression.ArrayIndex(a, Expression.Constant(i)), p)).ToArray();
                Expression call = Expression.Invoke(Expression.Convert(d, delegateType), each);
                _invoke = Expression.Lambda<Action<Delegate, object?[]>>(call, d, a).Compile();
            }

            /// <summary>What a raise of the event <paramref name="name"/> invokes its handlers with.</summary>
            internal object?[] Arguments(string name, object sender, object?[] given)
            {
                if (_refused != null) throw new PwrsException($"the event {name} has the delegate type {Delegate.FullName}, which {_refused}");
                if (SenderFirst)
                {
                    object? e = given.Length switch
                    {
                        0 => Parameters[1] == typeof(EventArgs) ? EventArgs.Empty : null,
                        1 => given[0],
                        _ => throw new PwrsException($"the event {name} is raised with the object as its sender and one argument of type {Parameters[1].FullName}, and {given.Length} arguments were given"),
                    };
                    return new[] { sender, Convert(e, Parameters[1]) };
                }
                if (given.Length != Parameters.Length)
                {
                    throw new PwrsException($"the event {name} takes {Parameters.Length} arguments ({string.Join(", ", Parameters.Select(p => p.FullName))}), and {given.Length} were given");
                }
                var args = new object?[given.Length];
                for (int i = 0; i < given.Length; i++) args[i] = Convert(given[i], Parameters[i]);
                return args;
            }

            internal void Invoke(Delegate handlers, object?[] args) => _invoke!(handlers, args);

            private static object? Convert(object? value, Type to) => LanguagePrimitives.ConvertTo(value, to);
        }
    }

    /// <summary>
    /// A raise queued on an object's <see cref="ProxyCore"/> by the thread
    /// inside a call into it, delivered when that thread's last entry
    /// leaves, with the handlers the event had when it was raised.
    /// </summary>
    internal sealed class QueuedRaise
    {
        internal readonly string Name;
        internal readonly object Sender;
        private readonly Delegate _handlers;
        private readonly ObjectEvents.Shape _shape;
        private readonly object?[] _args;

        internal QueuedRaise(string name, object sender, Delegate handlers, ObjectEvents.Shape shape, object?[] args)
        {
            Name = name;
            Sender = sender;
            _handlers = handlers;
            _shape = shape;
            _args = args;
        }

        /// <summary>
        /// Delivers <paramref name="raised"/> in the order raised, and stops
        /// at a handler's exception. That exception is thrown, unless the
        /// call that raised them failed, whose own error the caller then
        /// gets: the handler's is written to standard error, since the call
        /// has nowhere else to report it.
        /// </summary>
        internal static void DeliverAll(List<QueuedRaise>? raised, bool callFailed)
        {
            if (raised == null) return;
            foreach (QueuedRaise r in raised)
            {
                try
                {
                    r._shape.Invoke(r._handlers, r._args);
                }
                catch (Exception e) when (callFailed)
                {
                    Console.Error.WriteLine($"pwrs: a handler of the {r.Name} event of a {r.Sender.GetType().FullName} threw after the call that raised it failed: {e.GetType().FullName}: {e.Message}");
                    return;
                }
            }
        }
    }

    /// <summary>
    /// A Rust callback subscribed to an event of an object, or a static
    /// event of a type, through a handler of the event's own delegate type
    /// built for it; or to an event of a COM object whose type does not
    /// declare it, through a handler taking the event's arguments, attached
    /// by ComEventsHelper to the source interface the object's type
    /// information names. The handler calls the callback on the raising
    /// thread, as a C# handler added with += runs, with the sender and the
    /// arguments: for a delegate (object sender, T e) the sender and e, and
    /// for any other delegate, a COM event's included, no sender and every
    /// argument. The callback's error, or its panic, is thrown to the
    /// raiser.
    ///
    /// Removing the handler never waits for a call running on another
    /// thread. The callback's context is released once, after the handler
    /// is removed and the last call into it has returned, so no call
    /// outlives it and a call already running finishes.
    /// </summary>
    internal sealed unsafe class Subscription
    {
#if NET
        private readonly delegate* unmanaged[Cdecl]<IntPtr, IntPtr, IntPtr, IntPtr*, int> _callback;
        private readonly delegate* unmanaged[Cdecl]<IntPtr, void> _release;
#else
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int CallbackFn(IntPtr context, IntPtr sender, IntPtr args, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void ReleaseFn(IntPtr context);
        private readonly CallbackFn _callback;
        private readonly ReleaseFn _release;
#endif
        private readonly IntPtr _context;
        private readonly object? _target;
        private readonly string _name;
        /// <summary>The event, when the source's type declares it.</summary>
        private readonly EventInfo? _event;
        /// <summary>The event, when the source is a COM object whose type information names it.</summary>
        private readonly ComEvent? _com;
        /// <summary>The module's COM handlers, which record this one while it is attached to a COM object.</summary>
        private readonly ComHooks? _hooks;
        private readonly Delegate _handler;
        private readonly bool _senderFirst;
        /// <summary>Calls into the callback running now, on any thread.</summary>
        private int _calls;
        private int _removed;
        private int _released;

        /// <summary>
        /// Subscribes the callback to the event <paramref name="name"/> of
        /// <paramref name="source"/>: a static event when it is a Type that
        /// has one of that name, else an instance event, and on a COM object
        /// whose type does not declare it, the event its type information
        /// names, recorded in <paramref name="hooks"/> while it is attached. On
        /// success the subscription owns the context; on a throw the caller
        /// still does.
        /// </summary>
        internal Subscription(object source, string name, IntPtr callback, IntPtr context, IntPtr release, ComHooks hooks)
        {
            _name = name;
            EventInfo? found = null;
            if (source is Type type) found = type.GetEvent(name, BindingFlags.Public | BindingFlags.Static);
            if (found != null)
            {
                _target = null;
            }
            else
            {
                found = source.GetType().GetEvent(name, BindingFlags.Public | BindingFlags.Instance);
                _target = source;
            }
            string owner = source is Type t && _target == null ? t.FullName! : source.GetType().FullName!;
            Type delegateType;
            ParameterExpression[] parameters;
            if (found != null)
            {
                _event = found;
                delegateType = found.EventHandlerType ?? throw new PwrsException($"the event {name} of {owner} has no delegate type");
                ObjectEvents.Shape shape = ObjectEvents.ShapeOf(delegateType);
                if (shape.Parameters.Any(p => p.IsByRef))
                {
                    throw new PwrsException($"the event {name} of {owner} has the delegate type {delegateType.FullName}, which takes a ref, out or in argument, which a Rust callback is not given");
                }
                if (shape.Returns != typeof(void))
                {
                    throw new PwrsException($"the event {name} of {owner} has the delegate type {delegateType.FullName}, which returns {shape.Returns.FullName}, and a Rust callback returns nothing");
                }
                _senderFirst = shape.SenderFirst;
                parameters = shape.Parameters.Select((p, i) => Expression.Parameter(p, "p" + i)).ToArray();
            }
            else if (!(source is Type) && Marshal.IsComObject(source))
            {
                ComEvent com = ComEvent.Find(source, name);
                _com = com;
                _hooks = hooks;
                var signature = new Type[com.Arguments + 1];
                for (int i = 0; i < com.Arguments; i++) signature[i] = typeof(object);
                signature[com.Arguments] = typeof(void);
                delegateType = Expression.GetDelegateType(signature);
                parameters = Enumerable.Range(0, com.Arguments).Select(i => Expression.Parameter(typeof(object), "p" + i)).ToArray();
            }
            else
            {
                string events = string.Join(", ", source.GetType().GetEvents(BindingFlags.Public | BindingFlags.Instance).Select(e => e.Name));
                throw new PwrsException($"{owner} has no public event {name}; its events are {(events.Length == 0 ? "none" : events)}");
            }
            Expression all = Expression.NewArrayInit(typeof(object), parameters.Select(p => (Expression)Expression.Convert(p, typeof(object))));
            MethodInfo called = typeof(Subscription).GetMethod(nameof(Called), BindingFlags.Instance | BindingFlags.NonPublic)!;
            _handler = Expression.Lambda(delegateType, Expression.Call(Expression.Constant(this), called, all), parameters).Compile();
#if NET
            _callback = (delegate* unmanaged[Cdecl]<IntPtr, IntPtr, IntPtr, IntPtr*, int>)callback;
            _release = (delegate* unmanaged[Cdecl]<IntPtr, void>)release;
#else
            _callback = Marshal.GetDelegateForFunctionPointer<CallbackFn>(callback);
            _release = Marshal.GetDelegateForFunctionPointer<ReleaseFn>(release);
#endif
            _context = context;
            if (_com is ComEvent attach)
            {
                ComEventsHelper.Combine(source, attach.Interface, attach.DispId, _handler);
                hooks.Add(this);
            }
            else
            {
                _event!.AddEventHandler(_target, _handler);
            }
        }

        /// <summary>The handler's body: one call into the callback, counted while it runs.</summary>
        private void Called(object?[] args)
        {
            Interlocked.Increment(ref _calls);
            try
            {
                if (Volatile.Read(ref _removed) != 0) return;
                object? sender = _senderFirst ? args[0] : null;
                object?[] rest = _senderFirst ? new[] { args[1] } : args;
                IntPtr senderHandle = sender == null ? IntPtr.Zero : GCHandle.ToIntPtr(GCHandle.Alloc(sender));
                IntPtr argsHandle = GCHandle.ToIntPtr(GCHandle.Alloc(rest));
                IntPtr err = IntPtr.Zero;
                int status;
                try
                {
                    status = _callback(_context, senderHandle, argsHandle, &err);
                }
                finally
                {
                    if (senderHandle != IntPtr.Zero) GCHandle.FromIntPtr(senderHandle).Free();
                    GCHandle.FromIntPtr(argsHandle).Free();
                }
                object? errObj = Native.TakeErr(err);
                if (status != Native.Ok)
                {
                    throw errObj as Exception ?? new PwrsException(errObj as string ?? $"the Rust callback of the event {_name} failed with status {status}");
                }
            }
            finally
            {
                if (Interlocked.Decrement(ref _calls) == 0 && Volatile.Read(ref _removed) != 0) Release();
            }
        }

        /// <summary>
        /// Removes the handler, once, and a COM object's from the module's
        /// COM handlers. The context is released here when no call runs,
        /// and otherwise by the last call as it returns.
        /// </summary>
        internal void Remove()
        {
            if (Interlocked.Exchange(ref _removed, 1) != 0) return;
            try
            {
                if (_com is ComEvent attached)
                {
                    try
                    {
                        ComEventsHelper.Remove(_target!, attached.Interface, attached.DispId, _handler);
                    }
                    finally
                    {
                        _hooks!.Forget(this);
                    }
                }
                else
                {
                    _event!.RemoveEventHandler(_target, _handler);
                }
            }
            finally
            {
                if (Volatile.Read(ref _calls) == 0) Release();
            }
        }

        private void Release()
        {
            if (Interlocked.Exchange(ref _released, 1) == 0) _release(_context);
        }
    }
}
