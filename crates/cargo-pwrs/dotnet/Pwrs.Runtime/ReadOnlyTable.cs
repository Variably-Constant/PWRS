using System;
using System.Collections;

namespace Pwrs
{
    /// <summary>
    /// A table PowerShell reads through both `$t.key` and `$t['key']`
    /// and cannot write through either.
    ///
    /// The engine exposes a key as a property only for the
    /// non-generic <see cref="IDictionary"/>, and only when the type
    /// carries it on its public surface: `Hashtable` and
    /// `OrderedDictionary` answer `$t.key`, while
    /// `Dictionary&lt;,&gt;` and `ReadOnlyDictionary&lt;,&gt;` hold
    /// that interface explicitly and answer null. So this implements
    /// it publicly and refuses every mutating member instead.
    ///
    /// Both write forms reach the indexer, so one throw covers them.
    /// The refusal is an exception rather than a silent no-op,
    /// because a write that appears to succeed and does not is worse
    /// than the sharing it guards against.
    ///
    /// It holds the source rather than copying it, so enumeration
    /// keeps whatever order the source has and there is no cost
    /// proportional to the number of keys. A view, not a snapshot: a
    /// change made through the source is visible here.
    ///
    /// A value that is itself a dictionary comes back wrapped, from
    /// the indexer, from Values and from the enumerator, so a nested
    /// table cannot be written either. Each such read returns a new
    /// wrapper, so two reads of one key are equal by content and not
    /// by reference.
    /// </summary>
    public sealed class ReadOnlyTable : IDictionary
    {
        private readonly IDictionary _inner;

        public ReadOnlyTable(IDictionary source)
        {
            _inner = source ?? throw new ArgumentNullException(nameof(source));
        }

        private static NotSupportedException Refuse() =>
            new NotSupportedException("this table is read-only; it was handed over as a shared view, not a copy");

        /// <summary>
        /// Wraps a nested dictionary on its way out. Anything else is
        /// handed back as it is.
        /// </summary>
        internal static object? Guard(object? value) =>
            value is IDictionary d and not ReadOnlyTable ? new ReadOnlyTable(d) : value;

        public object? this[object key]
        {
            get { return Guard(_inner[key]); }
            set { throw Refuse(); }
        }

        public bool IsReadOnly => true;
        public bool IsFixedSize => true;
        public ICollection Keys => _inner.Keys;
        public int Count => _inner.Count;
        public bool IsSynchronized => _inner.IsSynchronized;
        public object SyncRoot => _inner.SyncRoot;

        public ICollection Values
        {
            get
            {
                var guarded = new ArrayList(_inner.Count);
                foreach (object? v in _inner.Values) guarded.Add(Guard(v));
                return guarded;
            }
        }

        public void Add(object key, object? value) => throw Refuse();
        public void Clear() => throw Refuse();
        public void Remove(object key) => throw Refuse();

        public bool Contains(object key) => _inner.Contains(key);
        public void CopyTo(Array array, int index) => _inner.CopyTo(array, index);

        public IDictionaryEnumerator GetEnumerator() => new GuardedEnumerator(_inner.GetEnumerator());
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();

        public override string ToString() => "ReadOnlyTable(" + _inner.Count + ")";

        /// <summary>
        /// Guards on the way out of an enumeration too. Without this
        /// a `foreach` reaches the unwrapped nested value and the
        /// guarantee has a hole in it.
        /// </summary>
        private sealed class GuardedEnumerator : IDictionaryEnumerator
        {
            private readonly IDictionaryEnumerator _inner;

            internal GuardedEnumerator(IDictionaryEnumerator inner) { _inner = inner; }

            public object Key => _inner.Key;
            public object? Value => Guard(_inner.Value);
            public DictionaryEntry Entry => new DictionaryEntry(_inner.Key, Guard(_inner.Value));
            public object Current => Entry;
            public bool MoveNext() => _inner.MoveNext();
            public void Reset() => _inner.Reset();
        }
    }
}
